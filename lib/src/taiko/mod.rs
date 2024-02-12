use std::path::PathBuf;

use alloy_primitives::{Address, Bytes, B256};
use alloy_sol_types::{sol, SolCall};
use anyhow::{bail, ensure, Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;
use zeth_primitives::{ethers::{from_ethers_h160, from_ethers_h256}, transactions::{ethereum::EthereumTxEssence, TxEssence}, withdrawal::Withdrawal};
use ethers_core::types::{Block, Transaction, H160, H256, U256, U64};
use ethers_core::types::{Transaction as EthersTransaction};
use crate::host::preflight::Preflight;
use crate::{builder::{TaikoStrategy, TkoTxExecStrategy}, consts::ChainSpec, host::provider::{new_cached_rpc_provider, new_file_provider, new_provider, new_rpc_provider, BlockQuery, ProofQuery, Provider, TxQuery}, 
input::Input, taiko::consts::{check_anchor_signature, ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT}};

use self::provider::TaikoProvider;

pub mod protocol_instance;
pub mod consts;
pub mod provider;

sol! {
    function anchor(
        bytes32 l1Hash,
        bytes32 l1SignalRoot,
        uint64 l1Height,
        uint32 parentGasUsed
    )
        external
    {}
}

#[inline]
pub fn decode_anchor(bytes: &[u8]) -> Result<anchorCall> {
    anchorCall::abi_decode(bytes, true)
        .context("Invalid anchor call")
} 

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
        uint96 amount;
        uint64 id;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadata {
        bytes32 l1Hash; // slot 1
        bytes32 difficulty; // slot 2
        bytes32 blobHash; //or txListHash (if Blob not yet supported), // slot 3
        bytes32 extraData; // slot 4
        bytes32 depositsHash; // slot 5
        address coinbase; // L2 coinbase, // slot 6
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp; // slot 7
        uint64 l1Height;
        uint24 txListByteOffset;
        uint24 txListByteSize;
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash; // slot 8
    }

    #[derive(Debug)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        bytes32 graffiti;
    }

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    event BlockProposed(
        uint256 indexed blockId,
        address indexed prover,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );

    #[derive(Debug)]
    struct TierProof {
        uint16 tier;
        bytes data;
    }

    function proposeBlock(bytes calldata params, bytes calldata txList) {}

    function proveBlock(uint64 blockId, bytes calldata input) {}
}


#[derive(Debug)]
pub struct TaikoSystemInfo {
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
    pub l2_block: Block<EthersTransaction>,
}

impl TaikoSystemInfo {
    pub fn new(
        tp: &mut TaikoProvider,
        l2_block_no: u64,
        prover: Address,
        graffiti: B256,
    ) -> Result<Self> {

        let l2_block = tp.get_l2_full_block(l2_block_no)?;
        let l2_parent_block = tp.get_l2_full_block(l2_block_no - 1)?;

        let (anchor_tx, anchor_call) = tp.get_anchor(&l2_block)?;
        
        let l1_block_no = anchor_call.l1Height;
        let l1_block = tp.get_l1_full_block(l1_block_no)?;
        let l1_next_block = tp.get_l1_full_block(l1_block_no + 1 )?;

        let (proposal_call, proposal_event) = tp.get_proposal(l1_block_no, l2_block_no)?;

        // 0. check anchor Tx
        tp.check_anchor_tx(&anchor_tx, &l2_block);

        // 1. check l2 parent gas used
        ensure!(l2_parent_block.gas_used == ethers_core::types::U256::from(anchor_call.parentGasUsed), "parentGasUsed mismatch");
        
        // 2. check l1 signal root
        let mut l1_signal_root;
        if let Some(l1_signal_service) = tp.l1_signal_service {
            let proof = tp.l1_provider.get_proof(&ProofQuery {
                block_no: l1_block_no,
                address: l1_signal_service.into_array().into(),
                indices: Default::default(),
            })?;
            l1_signal_root = from_ethers_h256(proof.storage_hash);
            ensure!(l1_signal_root == anchor_call.l1SignalRoot, "l1SignalRoot mismatch");
        } else {
            bail!("l1_signal_service not set");
        }
        
        // 3. check l1 block hash
        ensure!(l1_block.hash.unwrap() == ethers_core::types::H256::from(anchor_call.l1Hash.0), "l1Hash mismatch");

        let proof = tp.l2_provider.get_proof(&ProofQuery {
            block_no: l2_block_no,
            address: tp.l2_signal_service.expect("l2_signal_service not set").into_array().into(),
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
            l1_next_block,
            l2_block,
        };

        Ok(sys_info)
    }
}

#[derive(Debug, Clone)]
pub struct HostArgs {
    l1_cache: Option<PathBuf>, 
    l1_rpc: Option<String>, 
    l2_cache: Option<PathBuf>, 
    l2_rpc: Option<String>,
    prover: Address,
}

async fn init_taiko(
    args: HostArgs,
    l2_chain_spec: ChainSpec,
    l2_block_no: u64,
    graffiti: B256,
) -> Result<(Input<EthereumTxEssence>, TaikoSystemInfo)> {
    let mut tp = TaikoProvider::new(
        args.l1_cache.clone(),
        args.l1_rpc.clone(),
        args.l2_cache.clone(),
        args.l2_rpc.clone(),
    )?
    .with_prover(args.prover)
    .with_l2_spec(l2_chain_spec.clone())
    .with_contracts(|| {
        use crate::taiko::consts::testnet::*;
        (*L1_CONTRACT, *L2_CONTRACT, *L1_SIGNAL_SERVICE, *L2_SIGNAL_SERVICE)
    });
    
    let sys_info = TaikoSystemInfo::new(&mut tp, l2_block_no, args.prover, graffiti)?;
    tp.save()?;

    let preflight_result = tokio::task::spawn_blocking(move || {
        TaikoStrategy::run_preflight(l2_chain_spec, args.l2_cache, args.l2_rpc, l2_block_no)
    })
    .await?;
    let preflight_data = preflight_result.context("preflight failed")?;

    // Create the guest input from [Init]
    let input: Input<EthereumTxEssence> = preflight_data
        .clone()
        .try_into()
        .context("invalid preflight data")?;

    Ok((input, sys_info))
}