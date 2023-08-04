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
use zeth_primitives::{
    trie::{to_prefix, MptNode, MptNodeData, MptNodeReference},
    RlpBytes,
};

pub fn load_pointers(
    root: &MptNode,
    node_store: &mut HashMap<MptNodeReference, MptNode>,
) -> MptNode {
    let compact_node = match root.as_data() {
        MptNodeData::Null | MptNodeData::Digest(_) | MptNodeData::Leaf(_, _) => root.clone(),
        MptNodeData::Branch(children) => {
            let compact_children: Vec<Box<MptNode>> = children
                .iter()
                .map(|child| Box::new(load_pointers(child, node_store)))
                .collect();
            MptNodeData::Branch(compact_children.try_into().unwrap()).into()
        }
        MptNodeData::Extension(prefix, target) => MptNodeData::Extension(
            prefix.clone(),
            Box::new(MptNodeData::Digest(target.hash()).into()),
        )
        .into(),
    };
    if let MptNodeData::Digest(_) = compact_node.as_data() {
        // do nothing
    } else {
        node_store.insert(compact_node.reference(), compact_node.clone());
    }
    compact_node
}

pub fn resolve_pointers(
    root: &MptNode,
    node_store: &HashMap<MptNodeReference, MptNode>,
) -> MptNode {
    let result: MptNode = match root.as_data() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => root.clone(),
        MptNodeData::Branch(nodes) => {
            let node_list: Vec<_> = nodes
                .iter()
                .map(|n| Box::new(resolve_pointers(n, node_store)))
                .collect();
            MptNodeData::Branch(
                node_list
                    .try_into()
                    .expect("Could not convert vector to 16-element array."),
            )
            .into()
        }
        MptNodeData::Extension(prefix, node) => {
            MptNodeData::Extension(prefix.clone(), Box::new(resolve_pointers(node, node_store)))
                .into()
        }
        MptNodeData::Digest(digest) => {
            if let Some(node) = node_store.get(&MptNodeReference::Digest(*digest)) {
                resolve_pointers(node, node_store)
            } else {
                root.clone()
            }
        }
    };
    assert_eq!(
        root.hash(),
        result.hash(),
        "Invalid node resolution! {:?} ({:?})",
        root.to_rlp(),
        result.to_rlp(),
    );
    result
}

pub fn orphaned_pointers(node: &MptNode) -> Vec<MptNode> {
    let mut result = Vec::new();
    _orphaned_pointers(node, &mut result);
    result
}

fn _orphaned_pointers(node: &MptNode, res: &mut Vec<MptNode>) {
    match node.as_data() {
        MptNodeData::Null => {}
        MptNodeData::Branch(children) => {
            let unresolved_count = children.iter().filter(|n| !n.is_resolved()).count();
            if unresolved_count == 1 {
                let unresolved_index = children.iter().position(|n| !n.is_resolved()).unwrap();
                res.push(*children[unresolved_index].clone());
            }
            // Continue descent
            for child in children {
                _orphaned_pointers(child, res);
            }
        }
        MptNodeData::Leaf(_, _) => {}
        MptNodeData::Extension(_, target) => {
            _orphaned_pointers(target, res);
        }
        MptNodeData::Digest(_) => {}
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
            for i in 0..nibs.len() {
                res.push(MptNodeData::Leaf(to_prefix(&nibs[i..], true), value.clone()).into())
            }
        }
        MptNodeData::Extension(_, target) => {
            for i in 0..nibs.len() {
                res.push(
                    MptNodeData::Extension(to_prefix(&nibs[i..], false), target.clone()).into(),
                )
            }
        }
    };
    res
}
