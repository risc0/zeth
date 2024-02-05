//! Prepare Input for guest
use std::fmt::Debug;

use anyhow::{anyhow, bail, Context, Result};
use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256, U256, U64};
use serde_json::to_string;
use thiserror::Error as ThisError;
use tracing::info;
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    keccak,
    taiko::{
        deposits_hash, string_to_bytes32, BlockMetadata, EthDeposit, ProtocolInstance, Transition,
        ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT,
    },
    transactions::EthereumTransaction,
    withdrawal::Withdrawal,
    Address, TxHash, B256,
};

use super::{
    provider::{new_provider, BlockQuery, ProofQuery, ProposeQuery, Provider},
    provider_db, Init,
};
use crate::{
    block_builder::{BlockBuilder, NetworkStrategyBundle},
    consts::ChainSpec,
    input::Input,
    taiko::{utils::rlp_decode_list, Layer},
    EthereumTxEssence,
};

#[derive(Debug)]
pub struct TaikoExtra {
    pub l1_hash: B256,
    pub l1_height: u64,
    pub l2_tx_list: Vec<u8>,
    pub prover: Address,
    pub graffiti: B256,
    pub l1_signal_root: B256,
    pub l2_signal_root: B256,
    pub l2_withdrawals: Vec<Withdrawal>,
    pub block_proposed: BlockProposed,
    pub l1_next_block: Block<EthersTransaction>,
    pub l2_fini_block: Block<EthersTransaction>,
}

#[allow(clippy::too_many_arguments)]
pub fn get_taiko_initial_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    l1_cache_path: Option<String>,
    _l1_chain_spec: ChainSpec,
    l1_rpc_url: Option<String>,
    prover: Address,
    l2_cache_path: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    graffiti: B256,
) -> Result<(Init<EthereumTxEssence>, TaikoExtra)> {
    let (l2_provider, l2_init_block, mut l2_fini_block, l2_signal_root, l2_input) = fetch_data(
        "L2",
        l2_cache_path,
        l2_rpc_url,
        l2_block_no,
        l2_chain_spec.l2_signal_service.unwrap(),
        Layer::L2,
    )?;
    // Get anchor call parameters
    let anchorCall {
        l1Hash: anchor_l1_hash,
        l1SignalRoot: anchor_l1_signal_root,
        l1Height: l1_block_no,
        parentGasUsed: l2_parent_gas_used,
    } = decode_anchor_call(&l2_fini_block.transactions[0].input)?;

    let (mut l1_provider, _l1_init_block, l1_fini_block, l1_signal_root, _l1_input) = fetch_data(
        "L1",
        l1_cache_path,
        l1_rpc_url,
        l1_block_no,
        l2_chain_spec.l1_signal_service.unwrap(),
        Layer::L1,
    )?;

    let (propose_tx, block_metadata) = l1_provider.get_propose(&ProposeQuery {
        l1_contract: H160::from_slice(l2_chain_spec.l1_contract.unwrap().as_slice()),
        l1_block_no: l1_block_no + 1,
        l2_block_no,
    })?;

    let l1_next_block = l1_provider.get_full_block(&BlockQuery {
        block_no: l1_block_no + 1,
    })?;

    // save l1 data
    l1_provider.save()?;

    let proposeBlockCall {
        params: _,
        txList: l2_tx_list,
    } = decode_propose_block(&propose_tx.input)?;

    // 1. check l2 parent gas used
    if l2_init_block.gas_used != U256::from(l2_parent_gas_used) {
        return Err(anyhow!(
            "parent gas used mismatch, expect: {}, got: {}",
            l2_init_block.gas_used,
            l2_parent_gas_used
        ));
    }
    // 2. check l1 signal root
    if anchor_l1_signal_root != l1_signal_root {
        return Err(anyhow!(
            "l1 signal root mismatch, expect: {}, got: {}",
            anchor_l1_signal_root,
            l1_signal_root
        ));
    }
    // 3. check l1 block hash
    if Some(anchor_l1_hash) != l1_fini_block.hash.map(from_ethers_h256) {
        return Err(anyhow!(
            "l1 block hash mismatch, expect: {}, got: {:?}",
            anchor_l1_hash,
            l1_fini_block.hash
        ));
    }

    let extra = TaikoExtra {
        l1_hash: anchor_l1_hash,
        l1_height: l1_block_no,
        l2_tx_list,
        prover,
        graffiti,
        l1_signal_root,
        l2_signal_root,
        l2_withdrawals: l2_input.withdrawals.clone(),
        block_proposed: block_metadata,
        l1_next_block,
        l2_fini_block: l2_fini_block.clone(),
    };

    // rebuild transaction list by tx_list from l1 contract
    decode_and_precheck_block(&l2_chain_spec, &mut l2_fini_block, &extra)?;

    // execute transactions and get states
    let init = execute_data::<N>(
        l2_provider,
        l2_chain_spec,
        l2_init_block,
        l2_input,
        l2_fini_block,
    )?;
    Ok((init, extra))
}

// decode the block with anchor transaction and txlist from l1 contract, then precheck it
pub fn decode_and_precheck_block(
    l2_chain_spec: &ChainSpec,
    l2_fini: &mut Block<EthersTransaction>,
    extra: &TaikoExtra,
) -> Result<()> {
    let Some(anchor) = l2_fini.transactions.first().cloned() else {
        bail!("no anchor transaction found");
    };
    // - check anchor transaction
    precheck_anchor(l2_chain_spec, l2_fini, &anchor)
        .map_err(|e| anyhow!(e.to_string()))
        .with_context(|| "precheck anchor error")?;

    let mut txs: Vec<EthersTransaction> = vec![];
    // - tx list bytes must be less than MAX_TX_LIST_BYTES
    if extra.l2_tx_list.len() <= MAX_TX_LIST_BYTES {
        txs = rlp_decode_list(&extra.l2_tx_list).unwrap_or_else(|err| {
            tracing::error!("decode tx list error: {}", err);
            vec![]
        });
    } else {
        tracing::error!(
            "tx list bytes must be not more than MAX_TX_LIST_BYTES, got: {}",
            extra.l2_tx_list.len()
        );
    }
    // - tx list must be less than MAX_TX_LIST
    if txs.len() > MAX_TX_LIST {
        tracing::error!(
            "tx list must be not more than MAX_TX_LIST, got: {}",
            txs.len()
        );
        // reset to empty
        txs.clear();
    }
    // - patch anchor transaction into tx list instead of those from l2 node's
    // insert the anchor transaction into the tx list at the first position
    txs.insert(0, anchor);
    // reset transactions
    l2_fini.transactions = txs;
    Ok(())
}

#[derive(ThisError, Debug)]
pub enum AnchorError {
    #[error("anchor transaction type mismatch, expected: {expected}, got: {got}")]
    AnchorTypeMisMatch { expected: u8, got: u8 },

    #[error("anchor transaction from mismatch, expected: {expected}, got: {got:?}")]
    AnchorFromMisMatch {
        expected: Address,
        got: Option<Address>,
    },

    #[error("anchor transaction to mismatch, expected: {expected}, got: {got:?}")]
    AnchorToMisMatch {
        expected: Address,
        got: Option<Address>,
    },

    #[error("anchor transaction value mismatch, expected: {expected}, got: {got:?}")]
    AnchorValueMisMatch { expected: U256, got: U256 },

    #[error("anchor transaction gas limit mismatch, expected: {expected}, got: {got:?}")]
    AnchorGasLimitMisMatch { expected: U256, got: U256 },

    #[error("anchor transaction fee cap mismatch, expected: {expected:?}, got: {got:?}")]
    AnchorFeeCapMisMatch {
        expected: Option<U256>,
        got: Option<U256>,
    },

    #[error("anchor transaction signature mismatch, {msg}")]
    AnchorSignatureMismatch { msg: String },

    #[error("anchor transaction decode error")]
    Anyhow(#[from] anyhow::Error),
}

pub fn precheck_anchor(
    l2_chain_spec: &ChainSpec,
    l2_fini: &Block<EthersTransaction>,
    anchor: &EthersTransaction,
) -> Result<(), AnchorError> {
    let tx1559_type = U64::from(0x2);
    if anchor.transaction_type != Some(tx1559_type) {
        return Err(AnchorError::AnchorTypeMisMatch {
            expected: tx1559_type.as_u64() as u8,
            got: anchor.transaction_type.unwrap_or_default().as_u64() as u8,
        });
    }
    let tx: EthereumTransaction = anchor.clone().try_into()?;
    // verify transaction
    check_anchor_signature(&tx)?;
    // verify the transaction signature
    let from = from_ethers_h160(anchor.from);
    if from != *GOLDEN_TOUCH_ACCOUNT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *GOLDEN_TOUCH_ACCOUNT,
            got: Some(from),
        });
    }
    let Some(to) = anchor.to else {
        return Err(AnchorError::AnchorToMisMatch {
            expected: l2_chain_spec.l2_contract.unwrap(),
            got: None,
        });
    };
    let to = from_ethers_h160(to);
    if to != l2_chain_spec.l2_contract.unwrap() {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: l2_chain_spec.l2_contract.unwrap(),
            got: Some(to),
        });
    }
    if anchor.value != U256::zero() {
        return Err(AnchorError::AnchorValueMisMatch {
            expected: U256::zero(),
            got: anchor.value,
        });
    }
    if anchor.gas != U256::from(ANCHOR_GAS_LIMIT) {
        return Err(AnchorError::AnchorGasLimitMisMatch {
            expected: U256::from(ANCHOR_GAS_LIMIT),
            got: anchor.gas,
        });
    }
    // anchor's gas price should be the same as the block's
    if anchor.max_fee_per_gas != l2_fini.base_fee_per_gas {
        return Err(AnchorError::AnchorFeeCapMisMatch {
            expected: l2_fini.base_fee_per_gas,
            got: anchor.max_fee_per_gas,
        });
    }
    Ok(())
}

pub fn assemble_protocol_instance(extra: &TaikoExtra, header: &Header) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak::keccak(extra.l2_tx_list.as_slice()));
    let deposits: Vec<EthDeposit> = extra
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect();
    let deposits_hash = deposits_hash(&deposits);
    let extra_data = string_to_bytes32(&header.extra_data);
    //   meta.difficulty = meta.blobHash ^ bytes32(block.prevrandao * b.numBlocks *
    // block.number);
    let block_hash = tx_list_hash;
    let block_hash_h256: zeth_primitives::U256 = block_hash.into();
    let prevrando = if cfg!(feature = "pos") {
        from_ethers_h256(extra.l1_next_block.mix_hash.unwrap_or_default()).into()
    } else {
        from_ethers_u256(extra.l1_next_block.difficulty)
    };
    let difficulty = block_hash_h256
        ^ (prevrando
            * zeth_primitives::U256::from(header.number)
            * zeth_primitives::U256::from(extra.l1_next_block.number.unwrap_or_default().as_u64()));
    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let mut pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: extra.l2_signal_root,
            graffiti: extra.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: extra.l1_hash,
            difficulty: difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data.into(),
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: extra.l1_height,
            txListByteOffset: 0u32,
            txListByteSize: extra.l2_tx_list.len() as u32,
            minTier: extra.block_proposed.meta.minTier,
            blobUsed: extra.l2_tx_list.is_empty(),
            parentMetaHash: extra.block_proposed.meta.parentMetaHash,
        },
        prover: extra.prover,
    };
    verify(header, &mut pi, extra)?;
    Ok(pi)
}

pub fn verify(header: &Header, pi: &mut ProtocolInstance, extra: &TaikoExtra) -> Result<()> {
    use alloy_sol_types::SolValue;
    // check the block metadata
    if pi.block_metadata.abi_encode() != extra.block_proposed.meta.abi_encode() {
        return Err(anyhow!(
            "block metadata mismatch, expected: {:?}, got: {:?}",
            extra.block_proposed.meta,
            pi.block_metadata
        ));
    }
    // println!("Protocol instance Transition: {:?}", pi.transition);
    // check the block hash
    if Some(header.hash()) != extra.l2_fini_block.hash.map(from_ethers_h256) {
        let txs: Vec<EthereumTransaction> = extra
            .l2_fini_block
            .transactions
            .iter()
            .filter_map(|tx| tx.clone().try_into().ok())
            .collect();
        return Err(anyhow!(
            "block hash mismatch, expected: {}, got: {}",
            to_string(&txs).unwrap_or_default(),
            to_string(&header.transactions).unwrap_or_default(),
        ));
    }

    Ok(())
}

#[allow(clippy::type_complexity)]
pub fn fetch_data(
    annotation: &str,
    cache_path: Option<String>,
    rpc_url: Option<String>,
    block_no: u64,
    signal_service: Address,
    layer: Layer,
) -> Result<(
    Box<dyn Provider>,
    Block<H256>,
    Block<EthersTransaction>,
    B256,
    Input<EthereumTxEssence>,
)> {
    let mut provider = new_provider(cache_path, rpc_url)?;

    let fini_query = BlockQuery { block_no };
    match layer {
        Layer::L1 => {}
        Layer::L2 => {
            provider.batch_get_partial_blocks(&fini_query)?;
        }
    }
    // Fetch the initial block
    let init_block = provider.get_partial_block(&BlockQuery {
        block_no: block_no - 1,
    })?;

    info!(
        "Initial {} block: {:?} ({:?})",
        annotation,
        init_block.number.unwrap(),
        init_block.hash.unwrap()
    );

    // Fetch the finished block
    let fini_block = provider.get_full_block(&fini_query)?;

    info!(
        "Final {} block number: {:?} ({:?})",
        annotation,
        fini_block.number.unwrap(),
        fini_block.hash.unwrap()
    );
    info!("Transaction count: {:?}", fini_block.transactions.len());

    // Get l2 signal root by signal service
    let proof = provider.get_proof(&ProofQuery {
        block_no,
        address: H160::from_slice(signal_service.as_slice()),
        indices: Default::default(),
    })?;
    let signal_root = from_ethers_h256(proof.storage_hash);

    info!(
        "Final {} signal root: {:?} ({:?})",
        annotation,
        fini_block.number.unwrap(),
        signal_root,
    );

    // Create input
    let input = Input {
        beneficiary: fini_block.author.map(from_ethers_h160).unwrap_or_default(),
        gas_limit: from_ethers_u256(fini_block.gas_limit),
        timestamp: from_ethers_u256(fini_block.timestamp),
        extra_data: fini_block.extra_data.0.clone().into(),
        mix_hash: from_ethers_h256(fini_block.mix_hash.unwrap()),
        transactions: fini_block
            .transactions
            .clone()
            .into_iter()
            .map(|tx| tx.try_into().unwrap())
            .collect(),
        withdrawals: fini_block
            .withdrawals
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|w| w.try_into().unwrap())
            .collect(),
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: vec![],
        parent_header: init_block.clone().try_into()?,
        ancestor_headers: vec![],
        base_fee_per_gas: from_ethers_u256(fini_block.base_fee_per_gas.unwrap_or_default()),
    };

    Ok((provider, init_block, fini_block, signal_root, input))
}

pub fn execute_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    provider: Box<dyn Provider>,
    chain_spec: ChainSpec,
    init_block: Block<H256>,
    input: Input<EthereumTxEssence>,
    fini_block: Block<EthersTransaction>,
) -> Result<Init<EthereumTxEssence>> {
    // Create the provider DB
    let provider_db = provider_db::ProviderDb::new(provider, init_block.number.unwrap().as_u64());
    // Create the block builder, run the transactions and extract the DB
    let mut builder = BlockBuilder::new(&chain_spec, input)
        .with_db(provider_db)
        .prepare_header::<N::HeaderPrepStrategy>()?
        .execute_transactions::<N::TxExecStrategy>()?;
    let provider_db = builder.mut_db().unwrap();

    info!("Gathering inclusion proofs ...");

    // Gather inclusion proofs for the initial and final state
    let init_proofs = provider_db.get_initial_proofs()?;
    let fini_proofs = provider_db.get_latest_proofs()?;

    // Gather proofs for block history
    let history_headers = provider_db.provider.batch_get_partial_blocks(&BlockQuery {
        block_no: fini_block.number.unwrap().as_u64(),
    })?;
    // ancestors == history - current - parent
    let ancestor_headers = if history_headers.len() > 2 {
        history_headers
            .into_iter()
            .rev()
            .skip(2)
            .map(|header| {
                header
                    .try_into()
                    .expect("Failed to convert ancestor headers")
            })
            .collect()
    } else {
        vec![]
    };

    info!("Saving provider cache ...");

    // Save the provider cache
    provider_db.get_provider().save()?;
    info!("Provider-backed execution is Done!");
    // assemble init
    let transactions = fini_block
        .transactions
        .clone()
        .into_iter()
        .map(|tx| tx.try_into().unwrap())
        .collect();
    let withdrawals = fini_block
        .withdrawals
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.try_into().unwrap())
        .collect();

    let init = Init {
        db: provider_db.get_initial_db().clone(),
        init_block: init_block.try_into()?,
        init_proofs,
        fini_block: fini_block.try_into()?,
        fini_transactions: transactions,
        fini_withdrawals: withdrawals,
        fini_proofs,
        ancestor_headers,
    };
    Ok(init)
}
