use std::sync::Arc;

pub use alloy_primitives::*;
use alloy_providers::tmp::{HttpProvider, TempProvider};
pub use alloy_rlp as rlp;
use alloy_rpc_types::{
    Block as AlloyBlock, BlockTransactions, Filter, Transaction as AlloyRpcTransaction,
};
use alloy_sol_types::{SolCall, SolEvent};
use alloy_transport_http::Http;
use anyhow::{anyhow, bail, Result};
use c_kzg::{Blob, KzgCommitment};
use hashbrown::HashSet;
use reth_primitives::{
    constants::eip4844::MAINNET_KZG_TRUSTED_SETUP, eip4844::kzg_to_versioned_hash,
};
use serde::{Deserialize, Serialize};
use url::Url;
use zeth_lib::{
    builder::{prepare::TaikoHeaderPrepStrategy, BlockBuilder, TkoTxExecStrategy},
    input::{
        decode_anchor, proposeBlockCall, protocol_testnet::BlockProposed as TestnetBlockProposed,
        BlockProposed, GuestInput, TaikoGuestInput, TaikoProverData,
    },
    taiko_utils::{generate_transactions, get_contracts, to_header},
};
use zeth_primitives::mpt::proofs_to_tries;

use crate::host::provider_db::ProviderDb;

pub trait RlpBytes {
    /// Returns the RLP-encoding.
    fn to_rlp(&self) -> Vec<u8>;
}

impl<T> RlpBytes for T
where
    T: rlp::Encodable,
{
    #[inline]
    fn to_rlp(&self) -> Vec<u8> {
        let rlp_length = self.length();
        let mut out = Vec::with_capacity(rlp_length);
        self.encode(&mut out);
        debug_assert_eq!(out.len(), rlp_length);
        out
    }
}

pub fn preflight(
    l1_rpc_url: Option<String>,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    chain_spec_name: &str,
    prover_data: TaikoProverData,
    beacon_rpc_url: Option<String>,
) -> Result<GuestInput> {
    let http_l2 = Http::new(Url::parse(&l2_rpc_url.clone().unwrap()).expect("invalid rpc url"));
    let provider_l2: HttpProvider = HttpProvider::new(http_l2);

    let http_l1 = Http::new(Url::parse(&l1_rpc_url.clone().unwrap()).expect("invalid rpc url"));
    let provider_l1: HttpProvider = HttpProvider::new(http_l1);

    let block = get_block(&provider_l2, l2_block_no, true).unwrap();
    let parent_block = get_block(&provider_l2, l2_block_no - 1, false).unwrap();

    // Decode the anchor tx to find out which L1 blocks we need to fetch
    let anchor_tx = match &block.transactions {
        BlockTransactions::Full(txs) => txs[0].to_owned(),
        _ => unreachable!(),
    };
    let anchor_call = decode_anchor(anchor_tx.input.as_ref())?;
    // The L1 blocks we need
    let l1_state_block_no = anchor_call.l1Height;
    let l1_inclusion_block_no = l1_state_block_no + 1;

    println!("block.hash: {:?}", block.header.hash.unwrap());
    println!("block.parent_hash: {:?}", block.header.parent_hash);
    println!("anchor L1 block id: {:?}", anchor_call.l1Height);
    println!("anchor L1 state root: {:?}", anchor_call.l1SignalRoot);

    // Get the L1 state block header so that we can prove the L1 state root
    let l1_inclusion_block = get_block(&provider_l1, l1_inclusion_block_no, false).unwrap();
    let l1_state_block = get_block(&provider_l1, l1_state_block_no, false).unwrap();
    println!(
        "l1_state_root_block hash: {:?}",
        l1_state_block.header.hash.unwrap()
    );

    // Get the block proposal data
    let (proposal_tx, proposal_event) = get_log(
        l1_rpc_url.clone().unwrap(),
        chain_spec_name,
        l1_inclusion_block.header.hash.unwrap(),
        l2_block_no,
    )?;

    // Fetch the tx list
    let (tx_list, tx_blob_hash) = if proposal_event.meta.blobUsed {
        println!("blob active");
        let metadata = &proposal_event.meta;

        // Get the blob hashes attached to the propose tx
        let blob_hashs = proposal_tx.blob_versioned_hashes;
        assert!(blob_hashs.len() >= 1);
        // Currently the protocol enforces the first blob hash to be used
        let blob_hash = blob_hashs[0];
        // TODO: check _proposed_blob_hash with blob_hash if _proposed_blob_hash is not None

        // Get the blob data for this block
        let blobs = get_blob_data(&beacon_rpc_url.clone().unwrap(), l1_inclusion_block_no)?;
        assert!(blobs.data.len() > 0, "blob data not available anymore");
        // Get the blob data for the blob storing the tx list
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob: &&GetBlobData| {
                // calculate from plain blob
                blob_hash == &calc_blob_versioned_hash(&blob.blob)
            })
            .cloned()
            .collect::<Vec<GetBlobData>>();
        let blob_data = decode_blob_data(&tx_blobs[0].blob);
        // Extract the specified range at which the tx list is stored
        let offset = metadata.txListByteOffset as usize;
        let size = metadata.txListByteSize as usize;
        (
            blob_data.as_slice()[offset..(offset + size)].to_vec(),
            Some(blob_hash),
        )
    } else {
        // Get the tx list data directly from the propose transaction data
        let proposal_call = proposeBlockCall::abi_decode(&proposal_tx.input, false).unwrap();
        (proposal_call.txList.clone(), None)
    };

    // Create the transactions from the proposed tx list
    let transactions = generate_transactions(&tx_list, anchor_tx.clone());
    // Do a sanity check using the transactions returned by the node
    println!("Block transactions: {:?}", block.transactions.len());
    assert!(
        transactions.len() >= block.transactions.len(),
        "unexpected number of transactions"
    );

    // Create the input struct without the block data set
    let taiko_guest_input = TaikoGuestInput {
        chain_spec_name: chain_spec_name.to_string(),
        l1_header: to_header(&l1_state_block.header),
        tx_list,
        anchor_tx: serde_json::to_string(&anchor_tx).unwrap(),
        tx_blob_hash,
        block_proposed: proposal_event,
        prover_data,
    };
    let input = GuestInput {
        block_hash: block.header.hash.unwrap().0.try_into().unwrap(),
        beneficiary: block.header.miner,
        gas_limit: block.header.gas_limit.try_into().unwrap(),
        timestamp: block.header.timestamp.try_into().unwrap(),
        extra_data: block.header.extra_data.0.into(),
        mix_hash: block.header.mix_hash.unwrap(),
        withdrawals: block.withdrawals.unwrap_or_default(),
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        parent_header: to_header(&parent_block.header),
        ancestor_headers: Default::default(),
        base_fee_per_gas: block.header.base_fee_per_gas.unwrap().try_into().unwrap(),
        taiko: taiko_guest_input,
    };

    // Create the block builder, run the transactions and extract the DB
    let provider_db = ProviderDb::new(
        provider_l2,
        parent_block.header.number.unwrap().try_into().unwrap(),
    );
    let mut builder = BlockBuilder::new(&input)
        .with_db(provider_db)
        .prepare_header::<TaikoHeaderPrepStrategy>()?
        .execute_transactions::<TkoTxExecStrategy>()?;
    let provider_db: &mut ProviderDb = builder.mut_db().unwrap();

    // Construct the state trie and storage from the storage proofs.
    // Gather inclusion proofs for the initial and final state
    let parent_proofs = provider_db.get_initial_proofs()?;
    let proofs = provider_db.get_latest_proofs()?;
    let (state_trie, storage) =
        proofs_to_tries(input.parent_header.state_root, parent_proofs, proofs)?;

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

    // Add the collected data to the input
    Ok(GuestInput {
        parent_state_trie: state_trie,
        parent_storage: storage,
        contracts: contracts.into_iter().map(Bytes).collect(),
        ancestor_headers,
        ..input
    })
}

const BLOB_FIELD_ELEMENT_NUM: usize = 4096;
const BLOB_FIELD_ELEMENT_BYTES: usize = 32;
const BLOB_DATA_LEN: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;

fn decode_blob_data(blob: &str) -> Vec<u8> {
    let origin_blob = hex::decode(blob.to_lowercase().trim_start_matches("0x")).unwrap();
    let header: U256 = U256::from_be_bytes::<BLOB_FIELD_ELEMENT_BYTES>(
        origin_blob[0..BLOB_FIELD_ELEMENT_BYTES].try_into().unwrap(),
    ); // first element is the length
    let expected_len = header.as_limbs()[0] as usize;

    assert!(origin_blob.len() == BLOB_DATA_LEN);
    // the first 32 bytes is the length of the blob
    // every first 1 byte is reserved.
    assert!(expected_len <= (BLOB_FIELD_ELEMENT_NUM - 1) * (BLOB_FIELD_ELEMENT_BYTES - 1));
    let mut chunk: Vec<Vec<u8>> = Vec::new();
    let mut decoded_len = 0;
    let mut i = 1;
    while decoded_len < expected_len && i < BLOB_FIELD_ELEMENT_NUM {
        let segment_len = if expected_len - decoded_len >= 31 {
            31
        } else {
            expected_len - decoded_len
        };
        let segment = &origin_blob
            [i * BLOB_FIELD_ELEMENT_BYTES + 1..i * BLOB_FIELD_ELEMENT_BYTES + 1 + segment_len];
        i += 1;
        decoded_len += segment_len;
        chunk.push(segment.to_vec());
    }
    chunk.iter().flatten().cloned().collect()
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes = hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap();
    let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
    let blob = Blob::from_bytes(&blob_bytes).unwrap();
    let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
    let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0;
    version_hash
}

fn get_blob_data(beacon_rpc_url: &str, block_id: u64) -> Result<GetBlobsResponse> {
    let tokio_handle = tokio::runtime::Handle::current();
    tokio_handle.block_on(async {
        let url = format!(
            "{}/eth/v1/beacon/blob_sidecars/{}",
            beacon_rpc_url, block_id
        );
        let response = reqwest::get(url.clone()).await?;
        if response.status().is_success() {
            let blob_response: GetBlobsResponse = response.json().await?;
            Ok(blob_response)
        } else {
            Err(anyhow::anyhow!(
                "Request failed with status code: {}",
                response.status()
            ))
        }
    })
}

// Blob data from the beacon chain
// type Sidecar struct {
// Index                    string                   `json:"index"`
// Blob                     string                   `json:"blob"`
// SignedBeaconBlockHeader  *SignedBeaconBlockHeader `json:"signed_block_header"`
// KzgCommitment            string                   `json:"kzg_commitment"`
// KzgProof                 string                   `json:"kzg_proof"`
// CommitmentInclusionProof []string
// `json:"kzg_commitment_inclusion_proof"` }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetBlobData {
    pub index: String,
    pub blob: String,
    // pub signed_block_header: SignedBeaconBlockHeader, // ignore for now
    pub kzg_commitment: String,
    pub kzg_proof: String,
    pub kzg_commitment_inclusion_proof: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetBlobsResponse {
    pub data: Vec<GetBlobData>,
}

pub fn get_block(provider: &HttpProvider, block_number: u64, full: bool) -> Result<AlloyBlock> {
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

pub fn get_log(
    rpc_url: String,
    chain_name: &str,
    block_hash: B256,
    l2_block_no: u64,
) -> Result<(AlloyRpcTransaction, BlockProposed)> {
    let http = Http::new(Url::parse(&rpc_url).expect("invalid rpc url"));
    let provider: HttpProvider = HttpProvider::new(http);
    let tokio_handle = tokio::runtime::Handle::current();

    // Get the address that emited the event
    let l1_address = get_contracts(chain_name).unwrap().0;

    // Get the event signature (value can differ between chains)
    let event_signature = if chain_name == "testnet" {
        TestnetBlockProposed::SIGNATURE_HASH
    } else {
        BlockProposed::SIGNATURE_HASH
    };
    // Setup the filter to get the relevant events
    let filter = Filter::new()
        .address(l1_address)
        .at_block_hash(block_hash)
        .event_signature(event_signature);
    // Now fetch the events
    let logs = tokio_handle.block_on(async { provider.get_logs(filter).await })?;

    // Run over the logs returned to find the matching event for the specified L2 block number
    // (there can be multiple blocks proposed in the same block and even same tx)
    for log in logs {
        if chain_name == "testnet" {
            let event = TestnetBlockProposed::decode_log(
                &Log::new(log.address, log.topics, log.data).unwrap(),
                false,
            )
            .unwrap();
            if event.blockId == zeth_primitives::U256::from(l2_block_no) {
                let tx = tokio_handle
                    .block_on(async {
                        provider
                            .get_transaction_by_hash(log.transaction_hash.unwrap())
                            .await
                    })
                    .expect("could not find the propose tx");
                return Ok((tx, event.data.into()));
            }
        } else {
            let event = BlockProposed::decode_log(
                &Log::new(log.address, log.topics, log.data).unwrap(),
                false,
            )
            .unwrap();
            if event.blockId == zeth_primitives::U256::from(l2_block_no) {
                let tx = tokio_handle
                    .block_on(async {
                        provider
                            .get_transaction_by_hash(log.transaction_hash.unwrap())
                            .await
                    })
                    .expect("could not find the propose tx");
                return Ok((tx, event.data));
            }
        }
    }
    bail!("No BlockProposed event found for block {l2_block_no}");
}
