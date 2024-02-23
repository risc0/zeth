use alloc::{string::String, vec::Vec};
use std::path::PathBuf;

use alloy_primitives::{Address, B256};
use anyhow::{bail, ensure, Context, Result};
use ethers_core::types::Transaction as EthersTransaction;
use log::info;
use rlp::{Decodable, DecoderError, Rlp};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header, ethers::from_ethers_h256, transactions::ethereum::EthereumTxEssence,
};

use super::{provider::TaikoProvider, TaikoSystemInfo};
use crate::{
    builder::{prepare::EthHeaderPrepStrategy, BlockBuilder, TaikoStrategy, TkoTxExecStrategy},
    consts::ChainSpec,
    host::{
        preflight::{new_preflight_input, Data, Preflight},
        provider::{BlockQuery, ProofQuery},
        provider_db::ProviderDb,
    },
    input::Input,
    taiko::consts::{get_contracts, MAX_TX_LIST, MAX_TX_LIST_BYTES},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostArgs {
    pub l1_cache: Option<PathBuf>,
    pub l1_rpc: Option<String>,
    pub l2_cache: Option<PathBuf>,
    pub l2_rpc: Option<String>,
}

pub fn init_taiko(
    args: HostArgs,
    l2_chain_spec: ChainSpec,
    testnet: &str,
    l2_block_no: u64,
    graffiti: B256,
    prover: Address,
) -> Result<(Input<EthereumTxEssence>, TaikoSystemInfo)> {
    let mut tp = TaikoProvider::new(
        args.l1_cache.clone(),
        args.l1_rpc.clone(),
        args.l2_cache.clone(),
        args.l2_rpc.clone(),
    )?
    .with_prover(prover)
    .with_l2_spec(l2_chain_spec.clone())
    .with_contracts(|| get_contracts(testnet));

    let sys_info = derive_sys_info(&mut tp, l2_block_no, prover, graffiti)?;
    tp.save()?;

    let preflight_data =
        TaikoStrategy::run_preflight(l2_chain_spec, args.l2_cache, args.l2_rpc, l2_block_no)?;

    // Create the guest input from [Init]
    let input: Input<EthereumTxEssence> = preflight_data
        .clone()
        .try_into()
        .context("invalid preflight data")?;

    Ok((input, sys_info))
}

pub fn derive_sys_info(
    tp: &mut TaikoProvider,
    l2_block_no: u64,
    prover: Address,
    graffiti: B256,
) -> Result<TaikoSystemInfo> {
    let l2_block = tp.get_l2_full_block(l2_block_no)?;
    let l2_parent_block = tp.get_l2_full_block(l2_block_no - 1)?;

    let (anchor_tx, anchor_call) = tp.get_anchor(&l2_block)?;

    let l1_block_no = anchor_call.l1Height;
    let l1_block = tp.get_l1_full_block(l1_block_no)?;
    let l1_next_block = tp.get_l1_full_block(l1_block_no + 1)?;

    let (proposal_call, proposal_event) = tp.get_proposal(l1_block_no, l2_block_no)?;

    // 0. check anchor Tx
    tp.check_anchor_tx(&anchor_tx, &l2_block)?;

    // 1. check l2 parent gas used
    ensure!(
        l2_parent_block.gas_used == ethers_core::types::U256::from(anchor_call.parentGasUsed),
        "parentGasUsed mismatch"
    );

    // 2. check l1 signal root
    let Some(l1_signal_service) = tp.l1_signal_service else {
        bail!("l1_signal_service not set");
    };

    let proof = tp.l1_provider.get_proof(&ProofQuery {
        block_no: l1_block_no,
        address: l1_signal_service.into_array().into(),
        indices: Default::default(),
    })?;

    let l1_signal_root = from_ethers_h256(proof.storage_hash);

    ensure!(
        l1_signal_root == anchor_call.l1SignalRoot,
        "l1SignalRoot mismatch"
    );

    // 3. check l1 block hash
    ensure!(
        l1_block.hash.unwrap() == ethers_core::types::H256::from(anchor_call.l1Hash.0),
        "l1Hash mismatch"
    );

    let proof = tp.l2_provider.get_proof(&ProofQuery {
        block_no: l2_block_no,
        address: tp
            .l2_signal_service
            .expect("l2_signal_service not set")
            .into_array()
            .into(),
        indices: Default::default(),
    })?;
    let l2_signal_root = from_ethers_h256(proof.storage_hash);

    tp.l1_provider.save()?;
    tp.l2_provider.save()?;

    let sys_info = TaikoSystemInfo {
        l1_hash: anchor_call.l1Hash,
        l1_height: anchor_call.l1Height,
        l2_tx_list: proposal_call.txList,
        prover,
        graffiti,
        l1_signal_root,
        l2_signal_root,
        l2_withdrawals: l2_block
            .withdrawals
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|w| w.try_into().unwrap())
            .collect(),
        block_proposed: proposal_event,
        l1_next_block: l1_next_block
            .try_into()
            .expect("l1_next_block converstion failed"),
        l2_block: l2_block.try_into().expect("l2_block converstion failed"),
    };

    Ok(sys_info)
}

impl Preflight<EthereumTxEssence> for TaikoStrategy {
    fn run_preflight(
        chain_spec: ChainSpec,
        cache_path: Option<std::path::PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> Result<Data<EthereumTxEssence>> {
        let mut tp = TaikoProvider::new(None, None, cache_path, rpc_url)?;

        // Fetch the parent block
        let parent_block = tp.l2_provider.get_partial_block(&BlockQuery {
            block_no: block_no - 1,
        })?;

        info!(
            "Initial block: {:?} ({:?})",
            parent_block.number.unwrap(),
            parent_block.hash.unwrap()
        );
        let parent_header: Header = parent_block.try_into().context("invalid parent block")?;

        // Fetch the target block
        let mut block = tp.l2_provider.get_full_block(&BlockQuery { block_no })?;
        let (anchor_tx, anchor_call) = tp.get_anchor(&block)?;
        let (proposal_call, _) = tp.get_proposal(anchor_call.l1Height, block_no)?;

        let mut l2_tx_list: Vec<EthersTransaction> = rlp_decode_list(&proposal_call.txList)?;
        ensure!(
            proposal_call.txList.len() <= MAX_TX_LIST_BYTES,
            "tx list bytes must be not more than MAX_TX_LIST_BYTES"
        );
        ensure!(
            l2_tx_list.len() <= MAX_TX_LIST,
            "tx list size must be not more than MAX_TX_LISTs"
        );

        // TODO(Cecilia): reset to empty necessary if wrong?
        // tracing::log for particular reason instead of uniform error handling?
        // txs.clear();

        info!(
            "Inserted anchor {:?} in tx_list decoded from {:?}",
            anchor_tx.hash, proposal_call.txList
        );
        l2_tx_list.insert(0, anchor_tx);
        block.transactions = l2_tx_list;

        info!(
            "Final block number: {:?} ({:?})",
            block.number.unwrap(),
            block.hash.unwrap()
        );
        info!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = ProviderDb::new(tp.l2_provider, parent_header.number);

        // Create the input data
        let input = new_preflight_input(block.clone(), parent_header.clone())?;
        let transactions = input.transactions.clone();
        let withdrawals = input.withdrawals.clone();

        // Create the block builder, run the transactions and extract the DB
        let mut builder = BlockBuilder::new(&chain_spec, input)
            .with_db(provider_db)
            .prepare_header::<EthHeaderPrepStrategy>()?
            .execute_transactions::<TkoTxExecStrategy>()?;
        let provider_db = builder.mut_db().unwrap();

        info!("Gathering inclusion proofs ...");

        // Gather inclusion proofs for the initial and final state
        let parent_proofs = provider_db.get_initial_proofs()?;
        let proofs = provider_db.get_latest_proofs()?;

        // Gather proofs for block history
        let ancestor_headers = provider_db.get_ancestor_headers()?;

        info!("Saving provider cache ...");

        // Save the provider cache
        provider_db.get_provider().save()?;

        info!("Provider-backed execution is Done!");

        Ok(Data {
            db: provider_db.get_initial_db().clone(),
            parent_header,
            parent_proofs,
            header: block.try_into().context("invalid block")?,
            transactions,
            withdrawals,
            proofs,
            ancestor_headers,
        })
    }
}

fn rlp_decode_list<T>(bytes: &[u8]) -> Result<Vec<T>, DecoderError>
where
    T: Decodable,
{
    let rlp = Rlp::new(bytes);
    rlp.as_list()
}
