// Copyright 2024 RISC Zero, Inc.
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

use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::EIP1186AccountProofResponse;
use anyhow::Context;
use hashbrown::HashMap;
use zeth_core::keccak::keccak;
use zeth_core::mpt::{
    is_not_included, mpt_from_proof, parse_proof, resolve_nodes, shorten_node_path, MptNode,
    MptNodeReference,
};
use zeth_core::stateless::data::StorageEntry;

pub fn proofs_to_tries(
    state_root: B256,
    initialization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
    finalization_proofs: HashMap<Address, EIP1186AccountProofResponse>,
) -> anyhow::Result<(MptNode, HashMap<Address, StorageEntry>)> {
    // if no addresses are provided, return the trie only consisting of the state root
    if initialization_proofs.is_empty() {
        return Ok((state_root.into(), HashMap::new()));
    }

    let mut storage: HashMap<Address, StorageEntry> =
        HashMap::with_capacity(initialization_proofs.len());

    let mut state_nodes = HashMap::new();
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
            .with_context(|| format!("missing initialization_proof for address {:#}", &address))?;

        // assure that addresses can be deleted from the state trie
        add_orphaned_leafs(address, &finalization_proof.account_proof, &mut state_nodes)?;

        // if no slots are provided, return the trie only consisting of the storage root
        if initialization_proof.storage_proof.is_empty() {
            storage.insert(address, (initialization_proof.storage_hash.into(), vec![]));
            continue;
        }

        let mut storage_nodes = HashMap::new();
        let mut storage_root_node = MptNode::default();
        for storage_proof in &initialization_proof.storage_proof {
            let proof_nodes =
                parse_proof(&storage_proof.proof).context("invalid storage_proof encoding")?;
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
            add_orphaned_leafs(
                storage_proof.key.0,
                &storage_proof.proof,
                &mut storage_nodes,
            )?;
        }
        // create the storage trie, from all the relevant nodes
        let storage_trie = resolve_nodes(&storage_root_node, &storage_nodes);
        assert_eq!(storage_trie.hash(), finalization_proof.storage_hash);

        // convert the slots to a vector of U256
        let slots = initialization_proof
            .storage_proof
            .iter()
            .map(|p| U256::from_be_bytes(p.key.0 .0))
            .collect();
        storage.insert(address, (storage_trie, slots));
    }
    let state_trie = resolve_nodes(&state_root_node, &state_nodes);
    assert_eq!(state_trie.hash(), state_root);

    Ok((state_trie, storage))
}

/// Adds all the leaf nodes of non-inclusion proofs to the nodes.
pub fn add_orphaned_leafs(
    key: impl AsRef<[u8]>,
    proof: &[impl AsRef<[u8]>],
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) -> anyhow::Result<()> {
    if !proof.is_empty() {
        let proof_nodes = parse_proof(proof).context("invalid proof encoding")?;
        if is_not_included(&keccak(key), &proof_nodes)? {
            // add the leaf node to the nodes
            let leaf = proof_nodes.last().unwrap();
            shorten_node_path(leaf).into_iter().for_each(|node| {
                nodes_by_reference.insert(node.reference(), node);
            });
        }
    }

    Ok(())
}
