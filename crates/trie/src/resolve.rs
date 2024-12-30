// Copyright 2023, 2024 RISC Zero, Inc.
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

use crate::data::MptNodeData;
use crate::node::MptNode;
use crate::reference::MptNodeReference;
use crate::util;
use alloy_primitives::map::HashMap;
use alloy_rlp::Decodable;
use anyhow::{bail, Context};

/// Parses proof bytes into a vector of MPT nodes.
pub fn parse_proof(proof: &[impl AsRef<[u8]>]) -> anyhow::Result<Vec<MptNode<'static>>> {
    proof
        .iter()
        // .filter(|proof| !proof.as_ref().is_empty()) // this is a sign of a malformed proof
        .map(|buf| MptNode::decode(&mut buf.as_ref()))
        .collect::<Result<Vec<_>, _>>()
        .context("parse_proof")
}

/// Creates a Merkle Patricia trie from an EIP-1186 proof.
pub fn mpt_from_proof(proof_nodes: &[MptNode<'static>]) -> anyhow::Result<MptNode<'static>> {
    let mut next: Option<MptNode<'static>> = None;
    for (i, node) in proof_nodes.iter().enumerate().rev() {
        // there is nothing to replace for the last node
        let Some(replacement) = next else {
            next = Some(node.clone());
            continue;
        };

        // find the child that references the next node
        let replacement_digest = replacement.hash();
        let resolved: MptNode<'static> = match node.as_data().clone() {
            MptNodeData::Branch(mut children) => {
                children.iter_mut().flatten().for_each(|c| {
                    if c.hash() == replacement_digest {
                        *c = Box::new(replacement.clone().into());
                    }
                });
                if children.iter_mut().all(|child| match child {
                    None => !replacement.is_empty(),
                    Some(node) => node.hash() != replacement_digest,
                }) {
                    bail!("branch node {} does not reference the successor", i);
                }
                MptNodeData::Branch(children).into()
            }
            MptNodeData::Extension(prefix, child) => {
                if child.hash() != replacement_digest {
                    bail!("extension node {} does not reference the successor", i);
                }
                MptNodeData::Extension(prefix, Box::new(replacement.into())).into()
            }
            MptNodeData::Null | MptNodeData::Leaf(_, _) | MptNodeData::Digest(_) => {
                bail!("node {} has no children to replace", i);
            }
        };

        next = Some(resolved);
    }

    // the last node in the proof should be the root
    Ok(next.unwrap_or_default())
}

/// Verifies that the given proof is a valid proof of exclusion for the given key.
pub fn is_not_included(key: &[u8], proof_nodes: &[MptNode<'static>]) -> anyhow::Result<bool> {
    let proof_trie = mpt_from_proof(proof_nodes).context("invalid trie")?;
    // for valid proofs, the get must not fail
    let value = proof_trie.get(key).context("invalid trie")?;

    Ok(value.is_none())
}

/// Creates a new MPT trie where all the digests contained in `node_store` are resolved.
pub fn resolve_nodes(
    root: &MptNode<'static>,
    node_store: &HashMap<MptNodeReference, MptNode<'static>>,
) -> MptNode<'static> {
    let trie = match root.as_data() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => root.clone(),
        MptNodeData::Branch(children) => {
            let children: Vec<_> = children
                .iter()
                .map(|child| {
                    child.as_ref().map(|node| {
                        Box::new(resolve_nodes(&node.clone().to_rw(), node_store).into())
                    })
                })
                .collect();
            MptNodeData::Branch(children.try_into().unwrap()).into()
        }
        MptNodeData::Extension(prefix, target) => MptNodeData::Extension(
            prefix.clone(),
            Box::new(resolve_nodes(&target.clone().to_rw(), node_store).into()),
        )
        .into(),
        MptNodeData::Digest(_) => {
            if let Some(node) = node_store.get(&root.reference()) {
                resolve_nodes(node, node_store)
            } else {
                root.clone()
            }
        }
    };
    // the root hash must not change
    debug_assert_eq!(root.hash(), trie.hash());

    trie
}

/// Creates a new MPT trie where all the digests contained in `node_store` are resolved.
pub fn resolve_nodes_in_place<'a>(
    root: &mut MptNode<'a>,
    node_store: &HashMap<MptNodeReference, MptNode<'a>>,
) {
    let starting_hash = root.hash();
    let replacement = match root.as_data_mut() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => None,
        MptNodeData::Branch(children) => {
            for child in children.iter_mut().flatten() {
                resolve_nodes_in_place(child.as_mut_node().unwrap(), node_store);
            }
            None
        }
        MptNodeData::Extension(_, target) => {
            resolve_nodes_in_place(target.as_mut_node().unwrap(), node_store);
            None
        }
        MptNodeData::Digest(_) => node_store.get(&root.reference()),
    };
    if let Some(data) = replacement {
        root.data = data.data.clone();
        root.invalidate_ref_cache();
        resolve_nodes_in_place(root, node_store);
    }
    // the root hash must not change
    debug_assert_eq!(root.hash(), starting_hash);
}

/// Returns a list of all possible nodes that can be created by shortening the path of the
/// given node.
/// When nodes in an MPT are deleted, leaves or extensions may be extended. To still be
/// able to identify the original nodes, we create all shortened versions of the node.
pub fn shorten_node_path<'a>(node: &MptNode<'a>) -> Vec<MptNode<'a>> {
    let mut res = Vec::new();
    let nibs = node.nibs();
    match node.as_data() {
        MptNodeData::Null | MptNodeData::Branch(_) => {}
        MptNodeData::Leaf(_, value) => {
            for i in 0..=nibs.len() {
                res.push(MptNodeData::Leaf(nibs[i..].to_vec(), value.clone()).into())
            }
        }
        MptNodeData::Extension(_, child) => {
            for i in 0..=nibs.len() {
                res.push(
                    MptNodeData::Extension(util::to_encoded_path(&nibs[i..], false), child.clone())
                        .into(),
                )
            }
        }
        MptNodeData::Digest(_) => unreachable!(),
    };
    res
}
