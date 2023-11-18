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

use anyhow::{bail, Result};
use hashbrown::HashMap;
use zeth_primitives::trie::{to_encoded_path, MptNode, MptNodeData, MptNodeReference};

/// Creates a Merkle Patricia trie from an EIP-1186 proof.
pub fn mpt_from_proof(proof_nodes: &[MptNode]) -> Result<MptNode> {
    let mut next: Option<MptNode> = None;
    for (i, node) in proof_nodes.iter().enumerate().rev() {
        // there is nothing to replace for the last node
        let Some(replacement) = next else {
            next = Some(node.clone());
            continue;
        };

        // the next node must have a digest reference
        let MptNodeReference::Digest(ref child_ref) = replacement.reference() else {
            bail!("node {} in proof is not referenced by hash", i + 1);
        };
        // find the child that references the next node
        let resolved: MptNode = match node.as_data().clone() {
            MptNodeData::Branch(mut children) => {
                if let Some(child) = children.iter_mut().flatten().find(
                    |child| matches!(child.as_data(), MptNodeData::Digest(d) if d == child_ref),
                ) {
                    *child = Box::new(replacement);
                } else {
                    bail!("node {} does not reference the successor", i);
                }
                MptNodeData::Branch(children).into()
            }
            MptNodeData::Extension(prefix, child) => {
                if !matches!(child.as_data(), MptNodeData::Digest(d) if d == child_ref) {
                    bail!("node {} does not reference the successor", i);
                }
                MptNodeData::Extension(prefix, Box::new(replacement)).into()
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

/// Creates a new MPT trie where all the digests contained in `node_store` are resolved.
pub fn resolve_digests(trie: &MptNode, node_store: &HashMap<MptNodeReference, MptNode>) -> MptNode {
    let result: MptNode = match trie.as_data() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => trie.clone(),
        MptNodeData::Branch(children) => {
            let children: Vec<_> = children
                .iter()
                .map(|child| {
                    child
                        .as_ref()
                        .map(|node| Box::new(resolve_digests(node, node_store)))
                })
                .collect();
            MptNodeData::Branch(children.try_into().unwrap()).into()
        }
        MptNodeData::Extension(prefix, target) => MptNodeData::Extension(
            prefix.clone(),
            Box::new(resolve_digests(target, node_store)),
        )
        .into(),
        MptNodeData::Digest(digest) => {
            if let Some(node) = node_store.get(&MptNodeReference::Digest(*digest)) {
                resolve_digests(node, node_store)
            } else {
                trie.clone()
            }
        }
    };
    assert_eq!(trie.hash(), result.hash());
    result
}

pub fn shorten_key(node: &MptNode) -> Vec<MptNode> {
    let mut res = Vec::new();
    let nibs = node.nibs();
    match node.as_data() {
        MptNodeData::Null | MptNodeData::Branch(_) | MptNodeData::Digest(_) => {}
        MptNodeData::Leaf(_, value) => {
            for i in 0..=nibs.len() {
                res.push(MptNodeData::Leaf(to_encoded_path(&nibs[i..], true), value.clone()).into())
            }
        }
        MptNodeData::Extension(_, target) => {
            for i in 0..=nibs.len() {
                res.push(
                    MptNodeData::Extension(to_encoded_path(&nibs[i..], false), target.clone())
                        .into(),
                )
            }
        }
    };
    res
}
