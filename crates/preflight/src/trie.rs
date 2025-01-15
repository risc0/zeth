// Copyright 2024, 2025 RISC Zero, Inc.
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

use alloy::primitives::map::{AddressHashMap, HashMap};
use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::EIP1186AccountProofResponse;
use anyhow::Context;
use std::collections::VecDeque;
use std::iter;
use zeth_core::keccak::keccak;
use zeth_core::mpt::{
    is_not_included, mpt_from_proof, parse_proof, prefix_nibs, resolve_nodes,
    resolve_nodes_in_place, shorten_node_path, MptNode, MptNodeData, MptNodeReference,
};
use zeth_core::stateless::data::StorageEntry;

pub type TrieOrphan = (B256, B256);
pub type OrphanPair = (Vec<TrieOrphan>, Vec<(Address, TrieOrphan)>);
pub fn extend_proof_tries(
    state_trie: &mut MptNode,
    storage_tries: &mut AddressHashMap<StorageEntry>,
    initialization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
    finalization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
) -> anyhow::Result<OrphanPair> {
    // collected orphan data
    let mut state_orphans = Vec::new();
    let mut storage_orphans = Vec::new();
    // storage for encountered trie data
    let mut state_nodes = HashMap::default();
    for (address, initialization_proof) in initialization_proofs {
        // Create individual nodes from proof
        let proof_nodes = parse_proof(&initialization_proof.account_proof)
            .context("invalid account_proof encoding")?;
        // Ensure the trie is consistent
        mpt_from_proof(&proof_nodes).context("invalid account_proof")?;
        // Insert each node into the trie data store
        proof_nodes.into_iter().for_each(|node| {
            assert_eq!(node.size(), 1);
            state_nodes.insert(node.reference(), node);
        });
        // insert inaccessible storage trie
        if let alloy::primitives::map::Entry::Vacant(e) = storage_tries.entry(address) {
            e.insert(StorageEntry {
                storage_trie: initialization_proof.storage_hash.into(),
                slots: vec![],
            });
        }
        // storage for encountered storage trie data
        let mut storage_nodes = HashMap::default();
        for storage_proof in &initialization_proof.storage_proof {
            let proof_nodes = parse_proof(&storage_proof.proof)
                .context("extend_proof_tries/parse storage proof")?;
            mpt_from_proof(&proof_nodes).with_context(|| {
                format!("extend_proof_tries/ mpt from storage proof: {initialization_proof:?}")
            })?;
            // Load storage entry
            let storage_entry = storage_tries.get_mut(&address).unwrap();
            let storage_key = U256::from_be_bytes(storage_proof.key.0 .0);
            // Push the storage key if new
            if !storage_entry.slots.contains(&storage_key) {
                storage_entry.slots.push(storage_key);
            }
            // Load storage trie nodes into store
            proof_nodes.into_iter().for_each(|node| {
                storage_nodes.insert(node.reference(), node);
            });
        }

        // ensure that trie orphans are loaded
        let finalization_proof = finalization_proofs
            .get(&address)
            .with_context(|| format!("missing finalization proof for address {}", &address))?;
        if let Some(state_orphan) =
            add_orphaned_nodes(address, &finalization_proof.account_proof, &mut state_nodes)
                .with_context(|| format!("failed to add orphaned nodes for address {}", &address))?
        {
            state_orphans.push(state_orphan);
        }

        let mut potential_storage_orphans = Vec::new();
        for storage_proof in &finalization_proof.storage_proof {
            if let Some(storage_orphan) = add_orphaned_nodes(
                storage_proof.key.0,
                &storage_proof.proof,
                &mut storage_nodes,
            )
            .context("failed to add orphaned nodes")?
            {
                potential_storage_orphans.push(storage_orphan);
            }
        }

        let storage_entry = storage_tries.get_mut(&address).unwrap();
        // Load up newly found storage nodes
        resolve_nodes_in_place(&mut storage_entry.storage_trie, &storage_nodes);
        // validate storage orphans
        for (prefix, digest) in potential_storage_orphans {
            if let Some(node) = storage_nodes.get(&MptNodeReference::Digest(digest)) {
                if !node.is_digest() {
                    // this orphan node has been resolved
                    continue;
                }
            }
            // defer node resolution
            storage_orphans.push((address, (prefix, digest)));
        }
    }
    // Load up newly found state nodes
    resolve_nodes_in_place(state_trie, &state_nodes);
    let state_orphans = state_orphans
        .into_iter()
        .filter(|o| {
            state_nodes
                .get(&MptNodeReference::Digest(o.1))
                .map(|n| !n.is_digest())
                .unwrap_or_default()
        })
        .collect();

    Ok((state_orphans, storage_orphans))
}

pub fn proofs_to_tries(
    state_root: B256,
    initialization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
    finalization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
) -> anyhow::Result<(MptNode, HashMap<Address, StorageEntry>)> {
    // if no addresses are provided, return the trie only consisting of the state root
    if initialization_proofs.is_empty() {
        return Ok((state_root.into(), HashMap::default()));
    }

    let mut storage: HashMap<Address, StorageEntry> =
        HashMap::with_capacity_and_hasher(initialization_proofs.len(), Default::default());

    let mut state_nodes = HashMap::default();
    let mut state_root_node = MptNode::default();
    for (address, initialization_proof) in initialization_proofs {
        let proof_nodes = parse_proof(&initialization_proof.account_proof)
            .context("invalid account_proof encoding")?;
        mpt_from_proof(&proof_nodes).context("invalid account_proof")?;

        // the first node in the proof is the root
        if let Some(node) = proof_nodes.first() {
            state_root_node = node.clone();
        }

        proof_nodes.into_iter().for_each(|node| {
            state_nodes.insert(node.reference(), node);
        });

        let finalization_proof = finalization_proofs
            .get(&address)
            .with_context(|| format!("missing finalization proof for address {:#}", &address))?;

        // assure that addresses can be deleted from the state trie
        add_orphaned_nodes(address, &finalization_proof.account_proof, &mut state_nodes)?;

        // if no slots are provided, return the trie only consisting of the storage root
        if initialization_proof.storage_proof.is_empty() {
            storage.insert(
                address,
                StorageEntry {
                    storage_trie: initialization_proof.storage_hash.into(),
                    slots: vec![],
                },
            );
            continue;
        }

        let mut storage_nodes = HashMap::default();
        let mut storage_root_node = MptNode::default();
        for storage_proof in &initialization_proof.storage_proof {
            let proof_nodes = parse_proof(&storage_proof.proof).context("proofs_to_tries")?;
            mpt_from_proof(&proof_nodes).context("invalid storage_proof")?;

            // the first node in the proof is the root
            if let Some(node) = proof_nodes.first() {
                storage_root_node = node.clone();
            }

            proof_nodes.into_iter().for_each(|node| {
                storage_nodes.insert(node.reference(), node);
            });
        }

        // assure that slots can be deleted from the storage trie
        for storage_proof in &finalization_proof.storage_proof {
            add_orphaned_nodes(
                storage_proof.key.0,
                &storage_proof.proof,
                &mut storage_nodes,
            )?;
        }
        // create the storage trie, from all the relevant nodes
        let storage_trie = resolve_nodes(&storage_root_node, &storage_nodes);
        assert_eq!(storage_trie.hash(), initialization_proof.storage_hash);

        // convert the slots to a vector of U256
        let slots = initialization_proof
            .storage_proof
            .iter()
            .map(|p| U256::from_be_bytes(p.key.0 .0))
            .collect();
        storage.insert(
            address,
            StorageEntry {
                storage_trie,
                slots,
            },
        );
    }
    let state_trie = resolve_nodes(&state_root_node, &state_nodes);
    assert_eq!(state_trie.hash(), state_root);

    Ok((state_trie, storage))
}

/// Adds all the nodes of non-inclusion proofs to the nodes.
pub fn add_orphaned_nodes(
    key: impl AsRef<[u8]>,
    proof: &[impl AsRef<[u8]>],
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) -> anyhow::Result<Option<TrieOrphan>> {
    if !proof.is_empty() {
        let proof_nodes = parse_proof(proof).context("invalid proof encoding")?;
        let offset = keccak(key);
        if is_not_included(&offset, &proof_nodes)? {
            // extract inferrable orphans
            let node = proof_nodes.last().unwrap();
            shorten_node_path(node).into_iter().for_each(|node| {
                nodes_by_reference.insert(node.reference().as_digest(), node);
            });
            if let MptNodeData::Extension(_, target) = node.as_data() {
                return Ok(Some((
                    nibbles_to_digest(&proof_nodes_nibbles(&proof_nodes)),
                    target.hash(),
                )));
            }
        }
    }
    Ok(None)
}

pub fn proof_nodes_nibbles(proof_nodes: &[MptNode]) -> Vec<u8> {
    let mut nibbles = VecDeque::new();
    let mut last_child = proof_nodes.last().unwrap().reference().as_digest();
    for node in proof_nodes.iter().rev() {
        match node.as_data() {
            MptNodeData::Branch(children) => {
                for (i, child) in children.iter().enumerate() {
                    if let Some(child) = child {
                        if child.reference().as_digest() == last_child {
                            nibbles.push_front(i as u8);
                            break;
                        }
                    }
                }
            }
            MptNodeData::Leaf(prefix, _) | MptNodeData::Extension(prefix, _) => {
                prefix_nibs(prefix)
                    .into_iter()
                    .rev()
                    .for_each(|n| nibbles.push_front(n));
            }
            MptNodeData::Null | MptNodeData::Digest(_) => unreachable!(),
        }
        last_child = node.reference();
    }
    nibbles.into()
}

pub fn nibbles_to_digest(nibbles: &[u8]) -> B256 {
    let padding = 64 - nibbles.len();
    let padded: Vec<_> = nibbles
        .iter()
        .copied()
        .chain(iter::repeat(0u8).take(padding))
        .collect();
    let bytes: Vec<_> = padded
        .chunks_exact(2)
        .map(|byte| (byte[0] << 4) + byte[1])
        .collect();
    B256::from_slice(&bytes)
}
