// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    collections::HashSet,
    iter::{once, zip},
};

use anyhow::{Context, Result};
use ethers_core::types::{Bytes, EIP1186ProofResponse, H256};
use hashbrown::HashMap;
use log::info;
use revm::{
    primitives::{Address, B160, B256, U256},
    Database,
};
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    keccak::keccak,
    revm::to_revm_b256,
    transaction::Transaction,
    trie::{MptNode, MptNodeData, MptNodeReference, EMPTY_ROOT},
    withdrawal::Withdrawal,
};

use crate::{
    block_builder::BlockBuilder,
    consts::ETH_MAINNET_CHAIN_SPEC,
    execution::EthTxExecStrategy,
    host::{
        mpt::{orphaned_digests, resolve_digests, shorten_key},
        provider::{new_provider, BlockQuery},
    },
    input::{Input, StorageEntry},
    mem_db::MemDb,
    preparation::EthHeaderPrepStrategy,
};

pub mod mpt;
pub mod provider;
pub mod provider_db;

#[derive(Clone)]
pub struct Init {
    pub db: MemDb,
    pub init_block: Header,
    pub init_proofs: HashMap<B160, EIP1186ProofResponse>,
    pub fini_block: Header,
    pub fini_transactions: Vec<Transaction>,
    pub fini_withdrawals: Vec<Withdrawal>,
    pub fini_proofs: HashMap<B160, EIP1186ProofResponse>,
    pub ancestor_headers: Vec<Header>,
}

pub fn get_initial_data(
    cache_path: Option<String>,
    rpc_url: Option<String>,
    block_no: u64,
) -> Result<Init> {
    let mut provider = new_provider(cache_path, rpc_url)?;

    // Fetch the initial block
    let init_block = provider.get_partial_block(&BlockQuery {
        block_no: block_no - 1,
    })?;

    info!(
        "Initial block: {:?} ({:?})",
        init_block.number.unwrap(),
        init_block.hash.unwrap()
    );

    // Fetch the finished block
    let fini_block = provider.get_full_block(&BlockQuery { block_no })?;

    info!(
        "Final block number: {:?} ({:?})",
        fini_block.number.unwrap(),
        fini_block.hash.unwrap()
    );
    info!("Transaction count: {:?}", fini_block.transactions.len());

    // Create the provider DB
    let provider_db =
        crate::host::provider_db::ProviderDb::new(provider, init_block.number.unwrap().as_u64());

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
        parent_header: init_block.clone().try_into()?,
        ..Default::default()
    };

    // Create the block builder, run the transactions and extract the DB
    let mut builder = BlockBuilder::new(&ETH_MAINNET_CHAIN_SPEC, input)
        .with_db(provider_db)
        .prepare_header::<EthHeaderPrepStrategy>()?
        .execute_transactions::<EthTxExecStrategy>()?;
    let provider_db = builder.mut_db().unwrap();

    info!("Gathering inclusion proofs ...");

    // Gather inclusion proofs for the initial and final state
    let init_proofs = provider_db.get_initial_proofs()?;
    let fini_proofs = provider_db.get_latest_proofs()?;

    // Gather proofs for block history
    let ancestor_headers = provider_db.get_ancestor_headers()?;

    info!("Saving provider cache ...");

    // Save the provider cache
    provider_db.get_provider().save()?;

    info!("Provider-backed execution is Done!");

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

    Ok(Init {
        db: provider_db.get_initial_db().clone(),
        init_block: init_block.try_into()?,
        init_proofs,
        fini_block: fini_block.try_into()?,
        fini_transactions: transactions,
        fini_withdrawals: withdrawals,
        fini_proofs,
        ancestor_headers,
    })
}

#[derive(Debug)]
pub enum VerifyError {
    BalanceMismatch {
        rpc_value: U256,
        our_value: U256,
        difference: U256,
    },
    NonceMismatch {
        rpc_value: u64,
        our_value: u64,
    },
    CodeHashMismatch {
        rpc_value: B256,
        our_value: B256,
    },
    StorageMismatch {
        index: U256,
        rpc_value: U256,
        our_db_value: U256,
        our_trie_value: U256,
    },
    StorageRootMismatch {
        address: Address,
        rpc_value: B256,
        our_value: B256,
        first_delta: Option<String>,
        indices: usize,
    },
}

pub fn verify_state(
    mut fini_db: MemDb,
    fini_proofs: HashMap<B160, EIP1186ProofResponse>,
    mut storage_deltas: HashMap<Address, MptNode>,
) -> Result<HashMap<B160, Vec<VerifyError>>> {
    let mut errors = HashMap::new();
    let fini_storage_keys = fini_db.storage_keys();

    // Construct expected tries from fini proofs
    let (nodes_by_pointer, mut storage) = proofs_to_tries(fini_proofs.values().cloned().collect());
    storage
        .values_mut()
        .for_each(|(n, _)| *n = resolve_digests(n, &nodes_by_pointer));
    storage_deltas
        .values_mut()
        .for_each(|n| *n = resolve_digests(n, &nodes_by_pointer));

    for (address, indices) in fini_storage_keys {
        let mut address_errors = Vec::new();

        let account_proof = fini_proofs
            .get(&address)
            .with_context(|| format!("Proof not found: {}", address))?;
        // for deleted accounts, use the default to compare
        let account_info = fini_db.basic(address)?.unwrap_or_default();

        // Account balance
        {
            let rpc_value: U256 = account_proof.balance.into();
            let our_value: U256 = account_info.balance;
            if rpc_value != our_value {
                let difference = rpc_value.abs_diff(our_value);
                address_errors.push(VerifyError::BalanceMismatch {
                    rpc_value,
                    our_value,
                    difference,
                })
            }
        }

        // Nonce
        {
            let rpc_value: u64 = account_proof.nonce.as_u64();
            let our_value: u64 = account_info.nonce;
            if rpc_value != our_value {
                address_errors.push(VerifyError::NonceMismatch {
                    rpc_value,
                    our_value,
                })
            }
        }

        // Code hash
        {
            let rpc_value: B256 = account_proof.code_hash.into();
            let our_value: B256 = account_info.code_hash;
            if rpc_value != our_value {
                address_errors.push(VerifyError::CodeHashMismatch {
                    rpc_value,
                    our_value,
                })
            }
        }

        // Storage root
        {
            let storage_root_node = storage_deltas.get(&address).cloned().unwrap_or_default();
            let our_value = to_revm_b256(storage_root_node.hash());
            let rpc_value = account_proof.storage_hash.into();
            if rpc_value != our_value {
                let expected = storage
                    .get(&address)
                    .unwrap()
                    .0
                    .debug_rlp::<zeth_primitives::U256>();
                let found_pp = storage_root_node.debug_rlp::<zeth_primitives::U256>();
                let first_delta = zip(expected, found_pp)
                    .find(|(e, f)| e != f)
                    .map(|(e, f)| format!("Storage trie delta!\nEXPECTED:\t{}FOUND:\t{}", e, f));
                address_errors.push(VerifyError::StorageRootMismatch {
                    address,
                    rpc_value,
                    our_value,
                    first_delta,
                    indices: indices.len(),
                });
            }
        }

        // Storage
        {
            let storage_trie = storage_deltas.get(&address).cloned().unwrap_or_default();
            for index in indices {
                let storage_index = H256::from(index.to_be_bytes());
                let rpc_value = account_proof
                    .storage_proof
                    .iter()
                    .find(|&storage| storage_index == storage.key)
                    .expect("Could not find storage proof")
                    .value
                    .into();
                let our_db_value = fini_db.storage(address, index)?;
                let trie_index = keccak(storage_index.as_bytes());
                let our_trie_value = storage_trie.get_rlp(&trie_index)?.unwrap_or_default();
                if rpc_value != our_db_value || our_db_value != our_trie_value {
                    address_errors.push(VerifyError::StorageMismatch {
                        index,
                        rpc_value,
                        our_db_value,
                        our_trie_value,
                    })
                }
            }
        }

        if !address_errors.is_empty() {
            errors.insert(address, address_errors);
        }
    }

    Ok(errors)
}

fn proofs_to_tries(
    proofs: Vec<EIP1186ProofResponse>,
) -> (
    HashMap<MptNodeReference, MptNode>,
    HashMap<B160, StorageEntry>,
) {
    // construct the proof tries
    let mut nodes_by_reference = HashMap::new();
    let mut storage = HashMap::new();
    for proof in proofs {
        // parse the nodes of the account proof
        for bytes in &proof.account_proof {
            let mpt_node = MptNode::decode(bytes).expect("Failed to decode state proof");
            nodes_by_reference.insert(mpt_node.reference(), mpt_node);
        }

        // process the proof for each storage entry
        let mut root_node = None;
        for storage_proof in &proof.storage_proof {
            // parse the nodes of the storage proof and return the root node
            root_node = storage_proof
                .proof
                .iter()
                .rev()
                .map(|bytes| MptNode::decode(bytes).expect("Failed to decode storage proof"))
                .inspect(|node| drop(nodes_by_reference.insert(node.reference(), node.clone())))
                .last();
            // the hash of the root node should match the proof's storage hash
            assert_eq!(
                root_node.as_ref().map_or(EMPTY_ROOT, |n| n.hash()),
                from_ethers_h256(proof.storage_hash)
            );
        }

        let root_node = if let Some(root_node) = root_node {
            root_node
        } else if proof.storage_hash.0 == EMPTY_ROOT.0 {
            MptNode::default()
        } else {
            // if there are no storage proofs but the root is non-empty, create a dummy
            // as this is just the digest any tries to update this trie will fail
            MptNodeData::Digest(from_ethers_h256(proof.storage_hash)).into()
        };
        // collect all storage slots with a proof
        let slots = proof
            .storage_proof
            .into_iter()
            .map(|p| zeth_primitives::U256::from_be_bytes(p.key.into()))
            .collect();

        storage.insert(proof.address.into(), (root_node, slots));
    }
    (nodes_by_reference, storage)
}

fn resolve_orphans(
    nodes: &Vec<Bytes>,
    orphans: &mut HashSet<MptNodeReference>,
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) {
    for node in nodes {
        let mpt_node = MptNode::decode(node).expect("Failed to decode state proof");
        for potential_orphan in shorten_key(mpt_node) {
            let potential_orphan_hash = potential_orphan.reference();
            if orphans.remove(&potential_orphan_hash) {
                nodes_by_reference.insert(potential_orphan_hash, potential_orphan);
            }
        }
    }
}

impl From<Init> for Input {
    fn from(value: Init) -> Input {
        // construct the proof tries
        let (mut nodes_by_reference, mut storage) =
            proofs_to_tries(value.init_proofs.values().cloned().collect());
        // there should be a trie and a list of storage slots for every account
        assert_eq!(storage.len(), value.db.accounts_len());

        // collect the code from each account
        let mut contracts = HashMap::new();
        for account in value.db.accounts.values() {
            let code = account.info.code.clone().unwrap();
            if !code.is_empty() {
                contracts.insert(code.hash, code.bytecode);
            }
        }

        // extract the state trie
        let state_root = value.init_block.state_root;
        let state_trie = nodes_by_reference
            .remove(&MptNodeReference::Digest(state_root))
            .expect("State root node not found");
        assert_eq!(state_root, state_trie.hash());

        // identify orphaned digests, that could lead to issues when deleting nodes
        let mut orphans = HashSet::new();
        for root in storage.values().map(|v| &v.0).chain(once(&state_trie)) {
            let root = resolve_digests(root, &nodes_by_reference);
            orphans.extend(orphaned_digests(&root));
        }
        // resolve those orphans using the proofs of the final state
        for fini_proof in value.fini_proofs.values() {
            resolve_orphans(
                &fini_proof.account_proof,
                &mut orphans,
                &mut nodes_by_reference,
            );
            for storage_proof in &fini_proof.storage_proof {
                resolve_orphans(&storage_proof.proof, &mut orphans, &mut nodes_by_reference);
            }
        }

        // resolve the pointers in the state root node and all storage root nodes
        let state_trie = resolve_digests(&state_trie, &nodes_by_reference);
        storage
            .values_mut()
            .for_each(|(n, _)| *n = resolve_digests(n, &nodes_by_reference));

        info!(
            "The partial state trie consists of {} nodes",
            state_trie.size()
        );
        info!(
            "The partial storage tries consist of {} nodes",
            storage.values().map(|(n, _)| n.size()).sum::<usize>()
        );

        // Create the block builder input
        Input {
            parent_header: value.init_block,
            beneficiary: value.fini_block.beneficiary,
            gas_limit: value.fini_block.gas_limit,
            timestamp: value.fini_block.timestamp,
            extra_data: value.fini_block.extra_data.0.clone().into(),
            mix_hash: value.fini_block.mix_hash,
            transactions: value.fini_transactions,
            withdrawals: value.fini_withdrawals,
            parent_state_trie: state_trie,
            parent_storage: storage,
            contracts: contracts.into_values().map(|bytes| bytes.into()).collect(),
            ancestor_headers: value.ancestor_headers,
        }
    }
}
