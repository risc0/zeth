use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use ethers_core::types::{Transaction as EthersTransaction};
use log::info;
use rlp::Rlp;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header, ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256}, mpt::proofs_to_tries, transactions::ethereum::EthereumTxEssence, Address, Bytes, B256, U256
};
use anyhow::anyhow;
use hashbrown::HashSet;

use crate::{
    builder::{prepare::EthHeaderPrepStrategy, BlockBuilder, TkoTxExecStrategy},
    consts::ChainSpec,
    host::{
        provider::BlockQuery, provider_db::ProviderDb, taiko_provider::TaikoProvider
    },
    input::{GuestInput, TaikoProverData, TaikoSystemInfo},
    taiko_utils::{MAX_TX_LIST, MAX_TX_LIST_BYTES},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostArgs {
    pub l1_cache: Option<PathBuf>,
    pub l1_rpc: Option<String>,
    pub l2_cache: Option<PathBuf>,
    pub l2_rpc: Option<String>,
}

pub fn taiko_run_preflight(
    l1_rpc_url: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    l2_contracts: &str,
    prover_data: TaikoProverData,
) -> Result<GuestInput<EthereumTxEssence>> {
    let mut tp = TaikoProvider::new(
        None,
        l1_rpc_url.clone(),
        None,
        l2_rpc_url.clone(),
    )?;

    // Fetch the parent block
    let parent_block = tp.l2_provider.get_partial_block(&BlockQuery {
        block_no: l2_block_no - 1,
    })?;

    info!(
        "Initial block: {:?} ({:?})",
        parent_block.number.unwrap(),
        parent_block.hash.unwrap()
    );
    let parent_header: Header = parent_block.try_into().context("invalid parent block")?;

    // Fetch the target block
    let mut block = tp.l2_provider.get_full_block(&BlockQuery { block_no: l2_block_no })?;
    let (anchor_tx, anchor_call) = tp.get_anchor(&block)?;

    let l1_state_block_no = anchor_call.l1Height;
    let l1_inclusion_block_no = l1_state_block_no + 1;

    // Get the block proposal data
    let (proposal_call, proposal_event) = tp.get_proposal(l1_inclusion_block_no, l2_block_no, l2_contracts)?;

    // Make sure to also do the preflight on the tx_list transactions so we have the necessary data
    // for invalid transactions.
    let mut l2_tx_list: Vec<EthersTransaction> = Rlp::new(&proposal_call.txList).as_list()?;
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

    // Get the L1 state block header so that we can prove the L1 state root
    // Fetch the parent block
    let l1_state_block = tp.l1_provider.get_partial_block(&BlockQuery {
        block_no: l1_state_block_no,
    })?;

    let taiko_sys_info = TaikoSystemInfo {
        l1_header: l1_state_block.try_into().expect("Failed to convert ethers block to zeth block"),
        tx_list: proposal_call.txList,
        block_proposed: proposal_event,
        prover_data,
    };

    // convert each transaction
    let transactions = block
        .transactions
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.try_into()
                .map_err(|err| anyhow!("transaction {i} invalid: {err:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // convert each withdrawal
    let withdrawals = block
        .withdrawals
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.try_into()
                .with_context(|| format!("withdrawal {i} invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Create the input struct without the block data set
    let input = GuestInput {
        beneficiary: from_ethers_h160(block.author.context("author missing")?),
        gas_limit: from_ethers_u256(block.gas_limit),
        timestamp: from_ethers_u256(block.timestamp),
        extra_data: block.extra_data.0.into(),
        mix_hash: from_ethers_h256(block.mix_hash.context("mix_hash missing")?),
        transactions,
        withdrawals,
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        parent_header: parent_header.clone(),
        ancestor_headers: Default::default(),
        base_fee_per_gas: from_ethers_u256(
            block.base_fee_per_gas.context("base_fee_per_gas missing")?,
        ),
        taiko: taiko_sys_info,
    };

    // Create the provider DB
    let provider_db = ProviderDb::new(tp.l2_provider, parent_header.number);

    // Create the block builder, run the transactions and extract the DB
    let mut builder = BlockBuilder::new(&l2_chain_spec, input.clone())
        .with_db(provider_db)
        .prepare_header::<EthHeaderPrepStrategy>()?
        .execute_transactions::<TkoTxExecStrategy>()?;
    let provider_db: &mut ProviderDb = builder.mut_db().unwrap();

    info!("Gathering inclusion proofs ...");

    // Gather inclusion proofs for the initial and final state
    let parent_proofs = provider_db.get_initial_proofs()?;
    let proofs = provider_db.get_latest_proofs()?;

    // Gather proofs for block history
    let ancestor_headers = provider_db.get_ancestor_headers()?;

    // Get the contracts from the initial db.
    let mut contracts = HashSet::new();
    let initial_db = &provider_db.initial_db;
    for account in initial_db.accounts.values() {
        let code = &account.info.code;
        if let Some(code) = code {
            contracts.insert(code.bytecode.0.clone());
        }
    }

    // Construct the state trie and storage from the proofs.
    let (state_trie, storage) =
        proofs_to_tries(input.parent_header.state_root, parent_proofs, proofs)?;

    info!("Saving provider cache ...");

    // Save the provider cache
    //tp.save()?;

    info!("Provider-backed execution is Done!");

    Ok(GuestInput {
        parent_state_trie: state_trie,
        parent_storage: storage,
        contracts: contracts.into_iter().map(Bytes).collect(),
        ancestor_headers,
        ..input
    })
}
