use std::path::PathBuf;

use alloy_sol_types::SolCall;
use alloy_rpc_types::{Block as AlloyBlock, BlockTransactions};
use alloy_providers::tmp::{HttpProvider, TempProvider};
use alloy_transport_http::Http;
use url::Url;

use anyhow::{anyhow, ensure, Context, Result};
use ethers_core::types::Transaction as EthersTransaction;
use hashbrown::HashSet;
use log::info;
use reth_primitives::eip4844::kzg_to_versioned_hash;
use rlp::Rlp;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    builder::{prepare::TaikoHeaderPrepStrategy, BlockBuilder, TkoTxExecStrategy},
    input::{
        decode_propose_block_call_params, proposeBlockCall, BlockMetadata, GuestInput,
        TaikoGuestInput, TaikoProverData,
    },
    taiko_utils::{generate_transactions, generate_transactions_alloy, MAX_TX_LIST_BYTES, to_header},
};
use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    mpt::proofs_to_tries,
    transactions::ethereum::EthereumTxEssence,
    Bytes,
};

use super::provider::BlockQuery;
use crate::host::{provider::GetBlobData, provider_db::ProviderDb, taiko_provider::TaikoProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostArgs {
    pub l1_cache: Option<PathBuf>,
    pub l1_rpc: Option<String>,
    pub l2_cache: Option<PathBuf>,
    pub l2_rpc: Option<String>,
}

pub fn get_block_alloy(rpc_url: String, block_number: u64, full: bool) -> Result<AlloyBlock> {
    let http = Http::new(Url::parse(&rpc_url).expect("invalid rpc url"));
    let provider: HttpProvider = HttpProvider::new(http);

    //info!("Querying RPC for full block: {query:?}");

    let tokio_handle = tokio::runtime::Handle::current();

    let response = tokio_handle.block_on(async {
        provider
            .get_block_by_number((block_number).into(), full)
            .await
    })?;

    match response {
        Some(out) => Ok(out),
        None => Err(anyhow!("No data for {block_number:?}")),
    }
}

pub fn taiko_run_preflight(
    l1_rpc_url: Option<String>,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    chain_spec_name: &str,
    prover_data: TaikoProverData,
    beacon_rpc_url: Option<String>,
) -> Result<GuestInput<EthereumTxEssence>> {
    let mut tp = TaikoProvider::new(
        None,
        l1_rpc_url.clone(),
        None,
        l2_rpc_url.clone(),
        beacon_rpc_url,
    )?;

    // Fetch the parent block
    /*let parent_block = tp.l2_provider.get_partial_block(&BlockQuery {
        block_no: l2_block_no - 1,
    })?;

    println!("parent_block: {:?}", parent_block);

    info!(
        "Initial block: {:?} ({:?})",
        parent_block.number.unwrap(),
        parent_block.hash.unwrap()
    );
    let parent_header: Header = parent_block.try_into().context("invalid parent block")?;*/

    let parent_block = get_block_alloy(l2_rpc_url.clone().unwrap(), l2_block_no - 1, false).unwrap();
    println!("*** alloy parent block ***:{:?}", parent_block);

    let block_alloy = get_block_alloy(l2_rpc_url.clone().unwrap(), l2_block_no, true).unwrap();
    println!("*** alloy block ***:{:?}", block_alloy);

    // Fetch the target block
    let mut block_ethers = tp.l2_provider.get_full_block(&BlockQuery {
        block_no: l2_block_no,
    })?;
    let (anchor_tx_ethers, anchor_call_ethers) = tp.get_anchor(&block_ethers)?;
    let (anchor_tx_alloy, anchor_call_alloy) = tp.get_anchor_alloy(&block_alloy)?;
    println!("block.hash: {:?}", block_alloy.header.hash.unwrap());
    println!("block.parent_hash: {:?}", block_alloy.header.parent_hash);
    println!("block: {:?}", block_ethers);

    println!("anchor L1 block id: {:?}", anchor_call_ethers.l1Height);
    println!("anchor L1 state root: {:?}", anchor_call_ethers.l1SignalRoot);

    let l1_state_block_no = anchor_call_ethers.l1Height;
    let l1_inclusion_block_no = l1_state_block_no + 1;

    println!("l1_state_block_no: {:?}", l1_state_block_no);

    let l1_state_root_block_alloy = get_block_alloy(l1_rpc_url.clone().unwrap(), l1_state_block_no, false).unwrap();
    println!("*** alloy block ***:{:?}", l1_state_root_block_alloy);

    // Get the L1 state block header so that we can prove the L1 state root
    // Fetch the parent block
    /*let l1_state_root_block = tp.l1_provider.get_partial_block(&BlockQuery {
        block_no: l1_state_block_no,
    })?;*/
    // println!("l1_state_root_block: {:?}", l1_state_root_block);
    println!(
        "l1_state_root_block hash: {:?}",
        l1_state_root_block_alloy.header.hash.unwrap()
    );

    // let l1_propose_block = tp.l1_provider.get_partial_block(&BlockQuery {
    // block_no: l1_inclusion_block_no,
    // })?;
    // println!("l1_propose_block: {:?}", l1_propose_block);

    // Get the block proposal data
    let (proposal_tx, proposal_event) =
        tp.get_proposal(l1_inclusion_block_no, l2_block_no, chain_spec_name)?;

    println!("proposal: {:?}", proposal_event);

    let proposal_call = proposeBlockCall::abi_decode(&proposal_tx.input, false).unwrap();
    // .with_context(|| "failed to decode propose block call")?;

    // Make sure to also do the preflight on the tx_list transactions so we have the necessary
    // data for invalid transactions.
    ensure!(
        proposal_call.txList.len() <= MAX_TX_LIST_BYTES,
        "tx list bytes must be not more than MAX_TX_LIST_BYTES"
    );

    // blobUsed == (txList.length == 0) according to TaikoL1
    let blob_used = proposal_call.txList.is_empty();
    let (tx_list, tx_blob_hash) = if blob_used {
        println!("blob active");
        let metadata = decode_propose_block_call_params(&proposal_call.params)
            .expect("valid propose_block_call_params");
        println!("metadata: {:?}", metadata);

        let blob_hashs = proposal_tx.blob_versioned_hashes.unwrap();
        // TODO: multiple blob hash support
        assert!(blob_hashs.len() == 1);
        let blob_hash = blob_hashs[0];
        // TODO: check _proposed_blob_hash with blob_hash if _proposed_blob_hash is not None

        let blobs = tp.l1_provider.get_blob_data(l1_inclusion_block_no)?;
        assert!(blobs.data.len() > 0, "blob data not available anymore");
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob| blob_hash.as_fixed_bytes() == &calc_blob_hash(&blob.kzg_commitment))
            .cloned()
            .collect::<Vec<GetBlobData>>();
        let blob_data = decode_blob_data(&tx_blobs[0].blob);
        let offset = metadata.txListByteOffset as usize;
        let size = metadata.txListByteSize as usize;
        (
            blob_data.as_slice()[offset..(offset + size)].to_vec(),
            Some(from_ethers_h256(blob_hash)),
        )
    } else {
        (proposal_call.txList.clone(), None)
    };

    // println!("tx_list: {:?}", tx_list);

    // Create the transactions for the proposed tx list
    let transactions_ethers: Vec<EthersTransaction> = generate_transactions(&tx_list, anchor_tx_ethers.clone());


    let transactions_alloy = generate_transactions_alloy(&tx_list, anchor_tx_alloy.clone());

    println!("Block valid transactions: {:?}", block_alloy.transactions.len());
    assert!(
        transactions_ethers.len() >= block_alloy.transactions.len(),
        "unexpected number of transactions"
    );

    // Set the original transactions on the block
    //block_alloy.transactions = BlockTransactions::default();

    info!(
        "Final block number: {:?} ({:?})",
        block_alloy.header.number.unwrap(),
        block_alloy.header.hash.unwrap()
    );
    println!("Transaction count: {:?}", block_alloy.transactions.len());

    let taiko_guest_input = TaikoGuestInput {
        chain_spec_name: chain_spec_name.to_string(),
        l1_header: to_header(&l1_state_root_block_alloy.header),
        tx_list,
        anchor_tx: Some(anchor_tx_ethers.try_into().unwrap()),
        anchor_tx_alloy: serde_json::to_string(&anchor_tx_alloy).unwrap(),
        tx_blob_hash,
        block_proposed: proposal_event,
        prover_data,
    };

    // convert each transaction
    /*let transactions = transactions_ethers
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.try_into()
                .map_err(|err| anyhow!("transaction {i} invalid: {err:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;*/

    // convert each withdrawal
    let withdrawals = block_ethers
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
        block_hash: block_alloy.header.hash.unwrap().0.try_into().unwrap(),
        beneficiary: block_alloy.header.miner,
        gas_limit: block_alloy.header.gas_limit.try_into().unwrap(),
        timestamp: block_alloy.header.timestamp.try_into().unwrap(),
        extra_data: block_alloy.header.extra_data.0.into(),
        mix_hash: block_alloy.header.mix_hash.unwrap(),
        transactions: Vec::new(),
        withdrawals,
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        parent_header: to_header(&parent_block.header),
        ancestor_headers: Default::default(),
        base_fee_per_gas: block_alloy.header.base_fee_per_gas.unwrap().try_into().unwrap(),
        taiko: taiko_guest_input,
    };

    // Create the provider DB
    let provider_db = ProviderDb::new(tp.l2_provider, parent_block.header.number.unwrap().try_into().unwrap());

    println!("execute block");

    // Create the block builder, run the transactions and extract the DB
    let mut builder = BlockBuilder::new(&input)
        .with_db(provider_db)
        .prepare_header::<TaikoHeaderPrepStrategy>()?
        .execute_transactions::<TkoTxExecStrategy>()?;
    let provider_db: &mut ProviderDb = builder.mut_db().unwrap();

    info!("Gathering inclusion proofs ...");

    // Construct the state trie and storage from the storage proofs.
    // Gather inclusion proofs for the initial and final state
    let parent_proofs = provider_db.get_initial_proofs()?;
    let proofs = provider_db.get_latest_proofs()?;
    let (state_trie, storage) =
        proofs_to_tries(input.parent_header.state_root, parent_proofs, proofs)?;

    // Gather proofs for block history
    let ancestor_headers = provider_db.get_ancestor_headers(l2_rpc_url.unwrap())?;

    // Get the contracts from the initial db.
    let mut contracts = HashSet::new();
    let initial_db = &provider_db.initial_db;
    for account in initial_db.accounts.values() {
        let code = &account.info.code;
        if let Some(code) = code {
            contracts.insert(code.bytecode.0.clone());
        }
    }

    info!("Saving provider cache ...");

    // Save the provider cache
    // tp.save()?;

    info!("Provider-backed execution is Done!");

    Ok(GuestInput {
        parent_state_trie: state_trie,
        parent_storage: storage,
        contracts: contracts.into_iter().map(Bytes).collect(),
        ancestor_headers,
        ..input
    })
}

fn decode_blob_data(blob: &str) -> Vec<u8> {
    let origin_blob = hex::decode(blob.to_lowercase().trim_start_matches("0x")).unwrap();
    assert!(origin_blob.len() == 4096 * 32);
    let mut chunk: Vec<Vec<u8>> = Vec::new();
    let mut last_seg_found = false;
    for i in (0..4096).rev() {
        let segment = &origin_blob[i * 32 + 1..(i + 1) * 32];
        if segment.iter().any(|&x| x != 0) || last_seg_found {
            chunk.push(segment.to_vec());
            last_seg_found = true;
        }
    }
    chunk.reverse();
    chunk.iter().flatten().cloned().collect()
}

fn calc_blob_hash(commitment: &str) -> [u8; 32] {
    let commit_bytes = hex::decode(commitment.to_lowercase().trim_start_matches("0x")).unwrap();
    let kzg_commit = c_kzg::KzgCommitment::from_bytes(&commit_bytes).unwrap();
    let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0;
    version_hash
}
