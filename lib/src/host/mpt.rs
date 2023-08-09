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

use hashbrown::HashMap;
use zeth_primitives::trie::{to_encoded_path, MptNode, MptNodeData, MptNodeReference};

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

/// Returns all orphaned digests in the trie.
pub fn orphaned_digests(trie: &MptNode) -> Vec<MptNodeReference> {
    let mut result = Vec::new();
    orphaned_digests_internal(trie, &mut result);
    result
}

fn orphaned_digests_internal(trie: &MptNode, orphans: &mut Vec<MptNodeReference>) {
    match trie.as_data() {
        MptNodeData::Branch(children) => {
            // iterate over all digest children
            let mut digests = children.iter().flatten().filter(|node| node.is_digest());
            // if there is exactly one digest child, it is an orphan
            if let Some(orphan_digest) = digests.next() {
                if digests.next().is_none() {
                    orphans.push(orphan_digest.reference());
                }
            };
            // recurse
            children.iter().flatten().for_each(|child| {
                orphaned_digests_internal(child, orphans);
            });
        }
        MptNodeData::Extension(_, target) => {
            orphaned_digests_internal(target, orphans);
        }
        MptNodeData::Null | MptNodeData::Leaf(_, _) | MptNodeData::Digest(_) => {}
    }
}

pub fn shorten_key(node: MptNode) -> Vec<MptNode> {
    let mut res = Vec::new();
    let nibs = node.nibs();
    match node.as_data() {
        MptNodeData::Null | MptNodeData::Branch(_) | MptNodeData::Digest(_) => {
            res.push(node.clone())
        }
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
