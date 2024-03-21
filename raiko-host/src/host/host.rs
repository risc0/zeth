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
    consts::{get_network_spec, Network},
    input::{
        decode_anchor, proposeBlockCall, taiko_a6::BlockProposed as TestnetBlockProposed,
        BlockProposed, GuestInput, TaikoGuestInput, TaikoProverData,
    },
    taiko_utils::{generate_transactions, to_header},
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
    network: Network,
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
    let l1_state_block_no = anchor_call.l1BlockId;
    let l1_inclusion_block_no = l1_state_block_no + 1;

    println!("block.hash: {:?}", block.header.hash.unwrap());
    println!("block.parent_hash: {:?}", block.header.parent_hash);
    println!("anchor L1 block id: {:?}", anchor_call.l1BlockId);
    println!("anchor L1 state root: {:?}", anchor_call.l1StateRoot);

    // Get the L1 state block header so that we can prove the L1 state root
    let l1_inclusion_block = get_block(&provider_l1, l1_inclusion_block_no, false).unwrap();
    let l1_state_block = get_block(&provider_l1, l1_state_block_no, false).unwrap();
    println!(
        "l1_state_root_block hash: {:?}",
        l1_state_block.header.hash.unwrap()
    );

    // Get the block proposal data
    let (proposal_tx, proposal_event) = get_block_proposed_event(
        l1_rpc_url.clone().unwrap(),
        network,
        l1_inclusion_block.header.hash.unwrap(),
        l2_block_no,
    )?;

    // Fetch the tx list
    let (tx_list, tx_blob_hash) = if proposal_event.meta.blobUsed {
        println!("blob active");
        // Get the blob hashes attached to the propose tx
        let blob_hashs = proposal_tx.blob_versioned_hashes;
        assert!(blob_hashs.len() >= 1);
        // Currently the protocol enforces the first blob hash to be used
        let blob_hash = blob_hashs[0];
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
        assert!(!tx_blobs.is_empty());
        (decode_blob_data(&tx_blobs[0].blob), Some(blob_hash))
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
        l1_header: to_header(&l1_state_block.header),
        tx_list,
        anchor_tx: serde_json::to_string(&anchor_tx).unwrap(),
        tx_blob_hash,
        block_proposed: proposal_event,
        prover_data,
    };
    let input = GuestInput {
        network,
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
const BLOB_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;
const BLOB_VERSION_OFFSET: usize = 1;
const BLOB_ENCODING_VERSION: u8 = 0;
const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;

// decoding https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go
fn decode_blob_data(blob_str: &str) -> Vec<u8> {
    let blob_buf: Vec<u8> = match hex::decode(blob_str.to_lowercase().trim_start_matches("0x")) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };

    // check the version
    if blob_buf[BLOB_VERSION_OFFSET] != BLOB_ENCODING_VERSION {
        return Vec::new();
    }

    // decode the 3-byte big-endian length value into a 4-byte integer
    let output_len =
        ((blob_buf[2] as u32) << 16 | (blob_buf[3] as u32) << 8 | (blob_buf[4] as u32)) as usize;
    if output_len > MAX_BLOB_DATA_SIZE {
        return Vec::new();
    }

    // round 0 is special cased to copy only the remaining 27 bytes of the first field element
    // into the output due to version/length encoding already occupying its first 5 bytes.
    let mut output = [0; MAX_BLOB_DATA_SIZE];
    output[0..27].copy_from_slice(&blob_buf[5..32]);

    // now process remaining 3 field elements to complete round 0
    let mut opos: usize = 28; // current position into output buffer
    let mut ipos: usize = 32; // current position into the input blob
    let mut encoded_byte: [u8; 4] = [0; 4]; // buffer for the 4 6-bit chunks
    encoded_byte[0] = blob_buf[0];
    for encoded_byte_i in encoded_byte.iter_mut().skip(1) {
        (*encoded_byte_i, opos, ipos) =
            match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                Ok(res) => res,
                Err(_) => return Vec::new(),
            }
    }
    opos = reassemble_bytes(opos, &encoded_byte, &mut output);

    // in each remaining round we decode 4 field elements (128 bytes) of the input into 127
    // bytes of output
    for _ in 1..1024 {
        if opos < output_len {
            for encoded_byte_j in &mut encoded_byte {
                // save the first byte of each field element for later re-assembly
                (*encoded_byte_j, opos, ipos) =
                    match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                        Ok(res) => res,
                        Err(_) => return Vec::new(),
                    }
            }
            opos = reassemble_bytes(opos, &encoded_byte, &mut output)
        }
    }
    for otailing in output.iter().skip(output_len) {
        if *otailing != 0 {
            return Vec::new();
        }
    }
    for itailing in blob_buf.iter().take(BLOB_DATA_CAPACITY).skip(ipos) {
        if *itailing != 0 {
            return Vec::new();
        }
    }
    output[0..output_len].to_vec()
}

fn decode_field_element(
    b: &[u8],
    opos: usize,
    ipos: usize,
    output: &mut [u8],
) -> Result<(u8, usize, usize)> {
    // two highest order bits of the first byte of each field element should always be 0
    if b[ipos] & 0b1100_0000 != 0 {
        return Err(anyhow::anyhow!(
            "ErrBlobInvalidFieldElement: field element: {}",
            ipos
        ));
    }
    // copy(output[opos:], b[ipos+1:ipos+32])
    output[opos..opos + 31].copy_from_slice(&b[ipos + 1..ipos + 32]);
    Ok((b[ipos], opos + 32, ipos + 32))
}

fn reassemble_bytes(
    opos: usize,
    encoded_byte: &[u8; 4],
    output: &mut [u8; MAX_BLOB_DATA_SIZE],
) -> usize {
    // account for fact that we don't output a 128th byte
    let opos = opos - 1;
    let x = (encoded_byte[0] & 0b0011_1111) | ((encoded_byte[1] & 0b0011_0000) << 2);
    let y = (encoded_byte[1] & 0b0000_1111) | ((encoded_byte[3] & 0b0000_1111) << 4);
    let z = (encoded_byte[2] & 0b0011_1111) | ((encoded_byte[3] & 0b0011_0000) << 2);
    // put the re-assembled bytes in their appropriate output locations
    output[opos - 32] = z;
    output[opos - (32 * 2)] = y;
    output[opos - (32 * 3)] = x;
    opos
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes: Vec<u8> =
        hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap();
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

pub fn get_block_proposed_event(
    rpc_url: String,
    network: Network,
    block_hash: B256,
    l2_block_no: u64,
) -> Result<(AlloyRpcTransaction, BlockProposed)> {
    let http = Http::new(Url::parse(&rpc_url).expect("invalid rpc url"));
    let provider: HttpProvider = HttpProvider::new(http);
    let tokio_handle = tokio::runtime::Handle::current();

    // Get the address that emited the event
    let l1_address = get_network_spec(network).l1_contract.unwrap();

    // Get the event signature (value can differ between chains)
    let event_signature = if network == Network::TaikoA6 {
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
        if network == Network::TaikoA6 {
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use c_kzg::{Blob, KzgCommitment};
    use ethers_core::types::Transaction;
    use reth_primitives::{
        constants::eip4844::MAINNET_KZG_TRUSTED_SETUP,
        eip4844::kzg_to_versioned_hash,
        revm_primitives::kzg::{parse_kzg_trusted_setup, KzgSettings},
    };
    use zeth_lib::taiko_utils::decode_transactions;

    use super::*;

    fn calc_commit_versioned_hash(commitment: &str) -> [u8; 32] {
        let commit_bytes = hex::decode(commitment.to_lowercase().trim_start_matches("0x")).unwrap();
        let kzg_commit = c_kzg::KzgCommitment::from_bytes(&commit_bytes).unwrap();
        let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0;
        version_hash
    }

    #[test]
    fn test_parse_kzg_trusted_setup() {
        // check if file exists
        let b_file_exists = std::path::Path::new("../kzg_parsed_trust_setup").exists();
        assert!(b_file_exists);
        // open file as lines of strings
        let kzg_trust_setup_str = std::fs::read_to_string("../kzg_parsed_trust_setup").unwrap();
        let (g1, g2) = parse_kzg_trusted_setup(&kzg_trust_setup_str)
            .map_err(|e| {
                println!("error: {:?}", e);
                e
            })
            .unwrap();
        println!("g1: {:?}", g1.0.len());
        println!("g2: {:?}", g2.0.len());
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        // check if file exists
        let b_file_exists = std::path::Path::new("../kzg_parsed_trust_setup").exists();
        assert!(b_file_exists);
        // open file as lines of strings
        let kzg_trust_setup_str = std::fs::read_to_string("../kzg_parsed_trust_setup").unwrap();
        let (g1, g2) = parse_kzg_trusted_setup(&kzg_trust_setup_str)
            .map_err(|e| {
                println!("error: {:?}", e);
                e
            })
            .unwrap();
        let kzg_settings = KzgSettings::load_trusted_setup(&g1.0, &g2.0).unwrap();
        let blob = [0u8; 131072].into();
        let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
        assert_eq!(
            kzg_to_versioned_hash(kzg_commit).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    #[ignore]
    #[test]
    fn test_new_blob_decode() {
        let valid_blob_str = "\
            01000004b0f904adb8b502f8b283028c59188459682f008459682f028286b394\
            006700100000000000000000000000000001009980b844a9059cbb0000000000\
            0000000000000001670010000000000000000000000000000100990000000000\
            000000000000000000000000000000000000000000000000000001c080a0af40\
            093afa19e4b7256a209c71a902d33985c5655e580d5fbf36815e290b623177a0\
            19d4b4ccaa5497a47845016680c128b63e74e9d6a9756ebdeb2f78a65e0fa120\
            0001f802f901f483028c592e8459682f008459682f02832625a0941670010000\
            0b000000000000000000000000000280b90184fa233d0c000000000000000000\
            0000000000000000000000000000000000000000000000200000000000000000\
            000000000000000000000000000000000000000000007e7e0000000000000000\
            0000000014dc79964da2c08b23698b3d3cc7ca32193d99550000000000000000\
            0000000014dc79964da2c08b23698b3d3cc7ca32193d99550000000000000000\
            0000000000016700100000000000000000000000000001009900000000000000\
            0000000000000000000000000000000000000000000000000100000000000000\
            000000000000000000000000000000000000000000002625a000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            000000000000976ea74026e726554db657fa54763abd0c3a0aa9000000000000\
            0000000000000000000000000000000000000000000000000120000000000000\
            220000000000000000000000000000000000000000000000001243726f6e4a6f\
            102053656e64546f6b656e730000000000000000000000000000c080a0a99edd\
            2b13d5436cb0fe71b2ea4e69c2292fdc682ae54fe702cc36d6634dd0ba85a057\
            119f9297ca5ebd5402bd886405fe3aa8f8182438a9e56c1ef2a1ec0ae4a0acb9\
            00f802f901f483028c592f8459682f008459682f02832625a094167001000000\
            000000000000000000000000000280b90184fa233d0c00000000000000000000\
            0000000000000000000000000000000000000000000020000000000000000000\
            0000000000000000000000000000000000000000007e7e000000000000000000\
            00000014dc79964da2c08b23698b3d3cc7ca32193d9955000000000000000000\
            00000014dc79964da2c08b23698b3d3cc7ca32193d9955000000000000000000\
            0000000001670010000000000000000000000000000100990000000000000000\
            0000000000000000000000000000000000000000000000010000000000000000\
            0000000000000000000000000000000000000000002625a00000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000976ea74026e726554db657fa54763abd0c3a0aa900000000000000\
            0000000000000000000000000000000000000000000000012000000000000000\
            2000000000000000000000000000000000000000000000001243726f6e4a6f62\
            0053656e64546f6b656e730000000000000000000000000000c080a08f0a9757\
            35d78526f1339c69c2ed02df7a6d7cded10c74fb57398c11c1420526c2a0047f\
            003054d3d75d33120020872b6d5e0a4a05e47c50179bb9a8b866b7fb71b30000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            00000000000000000000000000000000";
        // println!("valid blob: {:?}", valid_blob_str);
        let blob_str = format!("{:0<262144}", valid_blob_str);
        let dec_blob = decode_blob_data(&blob_str);
        println!("dec blob tx len: {:?}", dec_blob.len());
        let txs = decode_transactions(&dec_blob);
        println!("dec blob tx: {:?}", txs);
        // assert_eq!(hex::encode(dec_blob), expected_dec_blob);
    }

    #[test]
    fn test_c_kzg_lib_commitment() {
        // check c-kzg mainnet trusted setup is ok
        let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
        let blob = [0u8; 131072].into();
        let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
        assert_eq!(
            kzg_to_versioned_hash(kzg_commit).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    // #[ignore]
    // #[tokio::test]
    // async fn test_propose_block() {
    // tokio::task::spawn_blocking(|| {
    // let l2_chain_spec = get_taiko_chain_spec("internal_devnet_a");
    // let mut l1_provider = new_provider(
    // None,
    // Some("https://localhost:8545".to_owned()),
    // Some("https://localhost:3500/".to_owned()),
    // )
    // .expect("bad provider");
    // let (propose_tx, block_metadata) = l1_provider
    // .get_propose(&ProposeQuery {
    // l1_contract: H160::from_slice(l2_chain_spec.l1_contract.unwrap().as_slice()),
    // l1_block_no: 6093,
    // l2_block_no: 1000,
    // })
    // .expect("bad get_propose");
    // println!("propose_tx: {:?}", propose_tx);
    // println!("block_metadata: {:?}", block_metadata);
    // })
    // .await
    // .unwrap();
    // }
    //
    // #[ignore]
    // #[tokio::test]
    // async fn test_fetch_blob_data_and_hash() {
    // tokio::task::spawn_blocking(|| {
    // let mut provider = new_provider(
    // None,
    // Some("https://l1rpc.internal.taiko.xyz/".to_owned()),
    // Some("https://l1beacon.internal.taiko.xyz/".to_owned()),
    // )
    // .expect("bad provider");
    // let blob_data = fetch_blob_data("http://localhost:3500".to_string(), 5).unwrap();
    // let blob_data = provider.get_blob_data(17138).unwrap();
    // println!("blob len: {:?}", blob_data.data[0].blob.len());
    // let dec_blob = decode_blob_data(&blob_data.data[0].blob);
    // println!("dec blob tx: {:?}", dec_blob.len());
    //
    // println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
    // let blob_hash = calc_commit_versioned_hash(&blob_data.data[0].kzg_commitment);
    // println!("blob hash {:?}", hex::encode(blob_hash));
    // })
    // .await
    // .unwrap();
    // }
    //
    // #[ignore]
    // #[tokio::test]
    // async fn test_fetch_and_verify_blob_data() {
    // tokio::task::spawn_blocking(|| {
    // let mut provider = new_provider(
    // None,
    // Some("https://l1rpc.internal.taiko.xyz".to_owned()),
    // Some("https://l1beacon.internal.taiko.xyz".to_owned()),
    // )
    // .expect("bad provider");
    // let blob_data = provider.get_blob_data(168).unwrap();
    // let blob_bytes: [u8; 4096 * 32] = hex::decode(
    // blob_data.data[0]
    // .blob
    // .to_lowercase()
    // .trim_start_matches("0x"),
    // )
    // .unwrap()
    // .try_into()
    // .unwrap();
    // let blob: Blob = blob_bytes.into();
    // let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
    // let kzg_commit: KzgCommitment =
    // KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
    // assert_eq!(
    // "0x".to_owned() + &kzg_commit.as_hex_string(),
    // blob_data.data[0].kzg_commitment
    // );
    // println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
    // let calc_versioned_hash =
    // calc_commit_versioned_hash(&blob_data.data[0].kzg_commitment); println!("blob hash
    // {:?}", hex::encode(calc_versioned_hash)); })
    // .await
    // .unwrap();
    // }
    //
    // #[ignore]
    // #[tokio::test]
    // async fn test_fetch_and_decode_blob_tx() {
    // let block_num = std::env::var("TAIKO_L2_BLOCK_NO")
    // .unwrap_or("94".to_owned())
    // .parse::<u64>()
    // .unwrap();
    // tokio::task::spawn_blocking(move || {
    // let mut provider = new_provider(
    // None,
    // Some("http://35.202.137.144:8545".to_owned()),
    // Some("http://35.202.137.144:3500".to_owned()),
    // )
    // .expect("bad provider");
    // let blob_data = provider.get_blob_data(block_num).unwrap();
    // println!("blob str len: {:?}", blob_data.data[0].blob.len());
    // let blob_bytes = decode_blob_data(&blob_data.data[0].blob);
    // println!("blob byte len: {:?}", blob_bytes.len());
    // println!("blob bytes {:?}", blob_bytes);
    // rlp decode blob tx
    // let txs: Vec<Transaction> = rlp_decode_list(&blob_bytes).unwrap();
    // println!("blob tx: {:?}", txs);
    // })
    // .await
    // .unwrap();
    // }

    #[ignore]
    #[test]
    fn json_to_ethers_blob_tx() {
        let response = "{
            \"blockHash\":\"0xa61eea0256aa361dfd436be11b0e276470413fbbc34b3642fbbf3b5d8d72f612\",
		    \"blockNumber\":\"0x4\",
		    \"from\":\"0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266\",
		    \"gas\":\"0xf4240\",
		    \"gasPrice\":\"0x5e92e74e\",
		    \"maxFeePerGas\":\"0x8b772ea6\",
		    \"maxPriorityFeePerGas\":\"0x3b9aca00\",
		    \"maxFeePerBlobGas\":\"0x2\",
		    \"hash\":\"0xdb3b11250a2332cc4944fa8022836bd32da43c34d4f2e9e1b246cfdbc5b4c60e\",
		    \"input\":\"0x11762da2\",
		    \"nonce\":\"0x1\",
		    \"to\":\"0x5fbdb2315678afecb367f032d93f642f64180aa3\",
		    \"transactionIndex\":\"0x0\",
		    \"value\":\"0x0\",
		    \"type\":\"0x3\",
            \"accessList\":[],
		    \"chainId\":\"0x7e7e\",
            \"blobVersionedHashes\":[\"0x012d46373b7d1f53793cd6872e40e801f9af6860ecbdbaa2e28df25937618c6f\",\"0x0126d296b606f85b775b12b8b4abeb3bdb88f5a50502754d598537ae9b7fb947\"],
            \"v\":\"0x0\",
		    \"r\":\"0xaba289efba8ef610a5b3b70b72a42fe1916640f64d7112ec0b89087bbc8fff5f\",
		    \"s\":\"0x1de067d69b79d28d0a3bd179e332c85b93cedbd299d9e205398c073a59633dcf\",
		    \"yParity\":\"0x0\"
        }";
        let tx: Transaction = serde_json::from_str(response).unwrap();
        println!("tx: {:?}", tx);
    }
}
