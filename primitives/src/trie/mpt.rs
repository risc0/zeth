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

extern crate alloc;

use alloc::boxed::Box;
use core::{cell::RefCell, fmt::Debug, iter, mem};

use alloy_primitives::B256;
use alloy_rlp::Encodable;
use anyhow::{bail, Context, Result};
use rlp::{Decodable, DecoderError, Prototype, Rlp};
use serde::{Deserialize, Serialize};

use crate::{keccak::keccak, trie::EMPTY_ROOT, RlpBytes};

/// The type and data of a node in a Merkle Patricia trie.
#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MptNodeData {
    /// Empty trie node.
    Null,
    /// Node with at most 16 children and a value.
    Branch([Box<MptNode>; 16], Vec<u8>), // todo: take this away
    /// Leaf node with a value.
    Leaf(Vec<u8>, Vec<u8>),
    /// Node with exactly one child.
    Extension(Vec<u8>, Box<MptNode>),
    /// Reference to a node by its hash.
    Digest(B256),
}

/// A node in a Merkle Patricia trie.
#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MptNode {
    data: MptNodeData,
    #[serde(skip)]
    cached_reference: RefCell<Option<MptNodeReference>>,
}

/// Reference of one node inside another node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MptNodeReference {
    /// Short encodings (less than 32 bytes).
    Bytes(Vec<u8>),
    /// Keccak hash of long encodings (not less than 32 bytes).
    Digest(B256),
}

impl From<B256> for MptNodeReference {
    fn from(digest: B256) -> Self {
        Self::Digest(digest)
    }
}

impl Default for MptNode {
    fn default() -> Self {
        Self {
            data: MptNodeData::Null,
            cached_reference: RefCell::new(None),
        }
    }
}

impl From<MptNodeData> for MptNode {
    fn from(value: MptNodeData) -> Self {
        Self {
            data: value,
            cached_reference: RefCell::new(None),
        }
    }
}

impl Encodable for MptNode {
    /// Encode the node into the `out` buffer.
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match &self.data {
            MptNodeData::Null => {
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            }
            MptNodeData::Branch(nodes, value) => {
                let mut payload_length = 0;
                for node in nodes {
                    payload_length += node.pointer_length();
                }
                payload_length += value.as_slice().length();
                alloy_rlp::Header {
                    list: true,
                    payload_length,
                }
                .encode(out);
                for node in nodes {
                    node.pointer_encode(out);
                }
                value.as_slice().encode(out);
            }
            MptNodeData::Leaf(prefix, value) => {
                let payload_length = prefix.as_slice().length() + value.as_slice().length();
                alloy_rlp::Header {
                    list: true,
                    payload_length,
                }
                .encode(out);
                prefix.as_slice().encode(out);
                value.as_slice().encode(out);
            }
            MptNodeData::Extension(prefix, node) => {
                let payload_length = prefix.as_slice().length() + node.pointer_length();
                alloy_rlp::Header {
                    list: true,
                    payload_length,
                }
                .encode(out);
                prefix.as_slice().encode(out);
                node.pointer_encode(out);
            }
            MptNodeData::Digest(digest) => {
                digest.encode(out);
            }
        }
    }
}

// TODO: migrate to alloy_rlp
impl Decodable for MptNode {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        match rlp.prototype()? {
            Prototype::Null | Prototype::Data(0) => Ok(MptNodeData::Null.into()),
            Prototype::List(2) => {
                let path: Vec<u8> = rlp.val_at(0)?;
                let prefix = path[0];
                if (prefix & (2 << 4)) == 0 {
                    let node: MptNode = Decodable::decode(&rlp.at(1)?)?;
                    Ok(MptNodeData::Extension(path, Box::new(node)).into())
                } else {
                    Ok(MptNodeData::Leaf(path, rlp.val_at(1)?).into())
                }
            }
            Prototype::List(17) => {
                let mut node_list = Vec::with_capacity(16);
                for node_rlp in rlp.iter().take(16) {
                    node_list.push(Box::new(Decodable::decode(&node_rlp)?));
                }
                let value = rlp.val_at(16)?;
                Ok(MptNodeData::Branch(node_list.try_into().unwrap(), value).into())
            }
            Prototype::Data(32) => {
                let bytes: Vec<u8> = rlp.as_val()?;
                Ok(MptNodeData::Digest(B256::from_slice(&bytes)).into())
            }
            _ => Err(DecoderError::Custom("Unknown MPT Node format!")),
        }
    }
}

impl MptNode {
    /// Clears the trie, replacing it with [MptNodeData::NULL].
    pub fn clear(&mut self) {
        self.data = MptNodeData::Null;
        self.invalidate_ref_cache();
    }

    /// Decodes an RLP-encoded [MptNode].
    pub fn decode(bytes: impl AsRef<[u8]>) -> Result<MptNode> {
        rlp::decode(bytes.as_ref()).context("rlp decode failed")
    }

    /// Returns the type and data of the node.
    pub fn as_data(&self) -> &MptNodeData {
        &self.data
    }

    /// Returns the 256-bit hash of the node.
    pub fn hash(&self) -> B256 {
        match self.data {
            MptNodeData::Null => EMPTY_ROOT,
            _ => match self.pointer() {
                MptNodeReference::Digest(digest) => digest,
                MptNodeReference::Bytes(bytes) => keccak(bytes).into(),
            },
        }
    }

    /// Returns the pointer of this node when referenced inside another node.
    pub fn pointer(&self) -> MptNodeReference {
        self.cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_pointer())
            .clone()
    }

    /// Encodes the pointer for this node into the `out` buffer.
    fn pointer_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_pointer())
        {
            MptNodeReference::Bytes(bytes) => out.put_slice(bytes),
            MptNodeReference::Digest(digest) => digest.encode(out),
        }
    }

    /// Returns the length of the encoded pointer for this node.
    fn pointer_length(&self) -> usize {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_pointer())
        {
            MptNodeReference::Bytes(bytes) => bytes.len(),
            MptNodeReference::Digest(digest) => digest.length(),
        }
    }

    fn calc_pointer(&self) -> MptNodeReference {
        match &self.data {
            MptNodeData::Null => MptNodeReference::Bytes(vec![alloy_rlp::EMPTY_STRING_CODE]),
            MptNodeData::Digest(digest) => MptNodeReference::Digest(*digest),
            _ => {
                let encoded = self.to_rlp();
                if encoded.len() < 32 {
                    MptNodeReference::Bytes(encoded)
                } else {
                    MptNodeReference::Digest(keccak(encoded).into())
                }
            }
        }
    }

    /// Returns whether the trie is empty.
    pub fn is_null(&self) -> bool {
        matches!(&self.data, MptNodeData::Null)
    }

    /// Returns whether the node is resolved or is the reference of another node.
    pub fn is_resolved(&self) -> bool {
        !matches!(&self.data, MptNodeData::Digest(_))
    }

    /// Returns the nibbles corresponding to the node's prefix.
    pub fn nibs(&self) -> Vec<u8> {
        match &self.data {
            MptNodeData::Null | MptNodeData::Branch(_, _) | MptNodeData::Digest(_) => Vec::new(),
            MptNodeData::Leaf(prefix, _) | MptNodeData::Extension(prefix, _) => {
                let extension = prefix[0];
                // the first bit of the first nibble denotes the parity
                let is_odd = extension & (1 << 4) != 0;

                let mut result = Vec::with_capacity(2 * prefix.len());
                // for odd lengths, the second nibble contains the first element
                if is_odd {
                    result.push(extension & 0xf);
                }
                for nib in &prefix[1..] {
                    result.push(nib >> 4);
                    result.push(nib & 0xf);
                }
                result
            }
        }
    }

    /// Returns the value of this node.
    pub fn value(&self) -> Option<&[u8]> {
        match &self.data {
            MptNodeData::Null | MptNodeData::Extension(_, _) | MptNodeData::Digest(_) => None,
            MptNodeData::Branch(_, value) | MptNodeData::Leaf(_, value) => Some(value),
        }
    }

    pub fn rlp_lookup<T: alloy_rlp::Decodable>(&self, trie_index: &[u8]) -> Result<Option<T>> {
        match self.lookup(trie_index) {
            Some(mut bytes) => Ok(Some(T::decode(&mut bytes).context("RLP decode failed")?)),
            None => Ok(None),
        }
    }

    fn lookup_internal(&self, key_nibs: &[u8]) -> Result<Option<&[u8]>> {
        match &self.data {
            MptNodeData::Null => Ok(None),
            MptNodeData::Branch(nodes, _) => {
                if key_nibs.is_empty() {
                    Ok(self.value())
                } else {
                    nodes
                        .get(key_nibs[0] as usize)
                        .unwrap()
                        .lookup_internal(&key_nibs[1..])
                }
            }
            MptNodeData::Leaf(_, _) => {
                if self.nibs() == key_nibs {
                    Ok(self.value())
                } else {
                    Ok(None)
                }
            }
            MptNodeData::Extension(_, target) => {
                let ext_nibs = self.nibs();
                let ext_len = ext_nibs.len();
                if key_nibs[..ext_len] != ext_nibs {
                    Ok(None)
                } else {
                    target.lookup_internal(&key_nibs[ext_len..])
                }
            }
            MptNodeData::Digest(_) => bail!("Cannot descend pointer!"),
        }
    }

    pub fn lookup(&self, trie_index: &[u8]) -> Option<&[u8]> {
        self.lookup_internal(&to_nibs(trie_index))
            .expect("Could not lookup value")
    }

    fn invalidate_ref_cache(&mut self) {
        self.cached_reference.borrow_mut().take();
    }

    fn delete_internal(&mut self, key_nibs: &[u8]) -> Result<bool> {
        let self_nibs = self.nibs();
        let value_deleted = match &mut self.data {
            MptNodeData::Null => false,
            MptNodeData::Branch(children, _) => {
                // note: we don't handle storing values at branches (key_nibs cannot be empty)
                let child = children.get_mut(key_nibs[0] as usize).unwrap();
                if !child.delete_internal(&key_nibs[1..])? {
                    return Ok(false);
                }
                let orphan_index = children
                    .iter()
                    .position(|n| !n.is_null())
                    .expect("Deleted last element of a branch!");
                let mut remaining_children: Vec<_> =
                    children.iter_mut().filter(|n| !n.is_null()).collect();
                if remaining_children.len() == 1 {
                    // convert to extension
                    let mut orphan = mem::take(remaining_children[0]);

                    let self_nibs = vec![orphan_index as u8];
                    let orphan_nibs = orphan.nibs();
                    match &mut orphan.data {
                        MptNodeData::Null => {
                            // Dead-end, delete own data
                            self.data = MptNodeData::Null;
                        }
                        MptNodeData::Branch(_, _) => {
                            self.data =
                                MptNodeData::Extension(to_prefix(&self_nibs, false), orphan);
                        }
                        MptNodeData::Leaf(_, orphan_data) => {
                            // Replace own data with extended-prefix leaf
                            let new_nibs = [self_nibs, orphan_nibs].concat();
                            self.data = MptNodeData::Leaf(
                                to_prefix(&new_nibs, true),
                                mem::take(orphan_data),
                            );
                        }
                        MptNodeData::Extension(_, orphan_target) => {
                            // Extend own prefix with child's
                            let new_nibs = [self_nibs, orphan_nibs].concat();
                            self.data = MptNodeData::Extension(
                                to_prefix(&new_nibs, false),
                                mem::take(orphan_target),
                            );
                        }
                        MptNodeData::Digest(_) => {
                            self.data =
                                MptNodeData::Extension(to_prefix(&self_nibs, false), orphan);
                        }
                    }
                }
                true
            }
            MptNodeData::Leaf(_, _) => {
                if self_nibs.len() != key_nibs.len() {
                    bail!(
                        "Unequal deletion key-lengths {}/{}",
                        self_nibs.len(),
                        key_nibs.len()
                    );
                }
                // Our traversal has ended
                if self_nibs == key_nibs {
                    self.data = MptNodeData::Null;
                    true
                } else {
                    false
                }
            }
            MptNodeData::Extension(_, child) => {
                let ext_len = self_nibs.len();
                if key_nibs[..ext_len] != self_nibs {
                    return Ok(false);
                }
                let value_deleted = child.delete_internal(&key_nibs[ext_len..])?;
                if value_deleted {
                    // Potentially collapse this branch
                    let child_nibs = child.nibs();
                    match &mut child.data {
                        MptNodeData::Branch(_, _) | MptNodeData::Digest(_) => {}
                        MptNodeData::Null => {
                            // Dead-end, delete own data
                            self.data = MptNodeData::Null;
                        }
                        MptNodeData::Leaf(_, child_value) => {
                            // Replace own data with extended-prefix leaf
                            let new_nibs = [self_nibs, child_nibs].concat();
                            self.data = MptNodeData::Leaf(
                                to_prefix(&new_nibs, true),
                                mem::take(child_value),
                            );
                        }
                        MptNodeData::Extension(_, child_target) => {
                            // Extend own prefix with child's
                            let new_nibs = [self_nibs, child_nibs].concat();
                            self.data = MptNodeData::Extension(
                                to_prefix(&new_nibs, false),
                                mem::take(child_target),
                            );
                        }
                    }
                }
                value_deleted
            }
            MptNodeData::Digest(_) => bail!("Hit a pointer during deletion operation!"),
        };

        if value_deleted {
            // invalidate the cache
            self.invalidate_ref_cache();
        }

        Ok(value_deleted)
    }

    pub fn delete(&mut self, trie_index: &[u8]) {
        self.delete_internal(&to_nibs(trie_index))
            .expect("Could not delete value");
    }

    fn update_internal(&mut self, key_nibs: &[u8], value: Vec<u8>) -> Result<bool> {
        let self_nibs = self.nibs();
        let value_updated = match &mut self.data {
            MptNodeData::Null => {
                self.data = MptNodeData::Leaf(to_prefix(key_nibs, true), value);
                true
            }
            MptNodeData::Branch(children, stored_value) => {
                if key_nibs.is_empty() {
                    // replace the branch data
                    *stored_value = value;
                    true
                } else {
                    children
                        .get_mut(key_nibs[0] as usize)
                        .unwrap()
                        .update_internal(&key_nibs[1..], value)?
                }
            }
            MptNodeData::Leaf(_, stored_value) => {
                let cpl = lcp(&self_nibs, key_nibs);
                if cpl == self_nibs.len() && cpl == key_nibs.len() {
                    // replace leaf data
                    let different_value = stored_value != &value;
                    if different_value {
                        *stored_value = value;
                    }
                    different_value
                } else {
                    let split_point = cpl + 1;
                    // create a branch with two children
                    let mut new_branch_children: [Box<MptNode>; 16] = Default::default();
                    let mut new_branch_value: Vec<u8> = Vec::new();
                    // Insert existing leaf data
                    if cpl == self_nibs.len() {
                        new_branch_value = mem::take(stored_value);
                    } else {
                        new_branch_children[self_nibs[cpl] as usize] = Box::new(
                            MptNodeData::Leaf(
                                to_prefix(&self_nibs[split_point..], true),
                                mem::take(stored_value),
                            )
                            .into(),
                        );
                    }
                    if cpl == key_nibs.len() {
                        new_branch_value = value;
                    } else {
                        new_branch_children[key_nibs[cpl] as usize] = Box::new(
                            MptNodeData::Leaf(to_prefix(&key_nibs[split_point..], true), value)
                                .into(),
                        );
                    }
                    let branch = MptNodeData::Branch(new_branch_children, new_branch_value);

                    if cpl > 0 {
                        // Create parent extension for new branch
                        self.data = MptNodeData::Extension(
                            to_prefix(&self_nibs[..cpl], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        self.data = branch;
                    }
                    true
                }
            }
            MptNodeData::Extension(_, existing_child) => {
                let cpl = lcp(&self_nibs, key_nibs);
                if cpl == self_nibs.len() {
                    // traverse down for update
                    existing_child.update_internal(&key_nibs[cpl..], value)?
                } else {
                    let split_point = cpl + 1;
                    // create a branch with two children
                    let mut new_branch_children: [Box<MptNode>; 16] = Default::default();
                    let mut new_branch_value: Vec<u8> = Vec::new();
                    // Insert existing extension
                    new_branch_children[self_nibs[cpl] as usize] = if self_nibs.len() > split_point
                    {
                        Box::new(
                            MptNodeData::Extension(
                                to_prefix(&self_nibs[split_point..], false),
                                mem::take(existing_child),
                            )
                            .into(),
                        )
                    } else {
                        mem::take(existing_child)
                    };
                    if cpl == key_nibs.len() {
                        new_branch_value = value;
                    } else {
                        new_branch_children[key_nibs[cpl] as usize] = Box::new(
                            MptNodeData::Leaf(to_prefix(&key_nibs[split_point..], true), value)
                                .into(),
                        );
                    }
                    let branch = MptNodeData::Branch(new_branch_children, new_branch_value);

                    if cpl > 0 {
                        // Create parent extension for new branch
                        self.data = MptNodeData::Extension(
                            to_prefix(&self_nibs[..cpl], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        self.data = branch;
                    }
                    true
                }
            }
            MptNodeData::Digest(_) => bail!("Hit a pointer during update operation!"),
        };

        if value_updated {
            // invalidate the cache
            self.invalidate_ref_cache();
        }

        Ok(value_updated)
    }

    pub fn rlp_update(&mut self, key: &[u8], value: impl Encodable) {
        self.update(key, value.to_rlp());
    }

    pub fn update(&mut self, trie_index: &[u8], value: Vec<u8>) {
        self.update_internal(&to_nibs(trie_index), value)
            .expect("Could not update value");
    }

    /// Formats the trie as string list, where each line corresponds to a trie leaf.
    pub fn rlp_debug<T: alloy_rlp::Decodable + Debug>(&self) -> Vec<String> {
        let nibs: String = self.nibs().iter().map(|n| format!("{:x}", n)).collect();
        match self.as_data() {
            MptNodeData::Null => vec![format!("{:?}", MptNodeData::Null)],
            MptNodeData::Branch(nodes, data) => {
                let c = nodes.iter().enumerate().flat_map(|(i, n)| {
                    n.rlp_debug::<T>()
                        .into_iter()
                        .map(move |s| format!("{:x} {}", i, s))
                });
                if data.is_empty() {
                    c.collect()
                } else {
                    iter::once(format!("-> [{:?}]", T::decode(&mut &data[..]).unwrap()))
                        .chain(c)
                        .collect()
                }
            }
            MptNodeData::Leaf(_, data) => {
                vec![format!(
                    "{} -> [{:?}]",
                    nibs,
                    T::decode(&mut &data[..]).unwrap()
                )]
            }
            MptNodeData::Extension(_, node) => node
                .rlp_debug::<T>()
                .into_iter()
                .map(|s| format!("{} {}", nibs, s))
                .collect(),
            MptNodeData::Digest(digest) => vec![format!("#{:#}", digest)],
        }
    }
}

fn lcp(a: &[u8], b: &[u8]) -> usize {
    let mut res = 0;
    while res < a.len() && res < b.len() {
        if a[res] != b[res] {
            break;
        }
        res += 1
    }
    res
}

pub fn to_nibs(slice: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(2 * slice.len());
    for nib in slice {
        result.push(nib >> 4);
        result.push(nib & 0xf);
    }
    result
}

pub fn to_prefix(nibs: &[u8], is_leaf: bool) -> Vec<u8> {
    let is_odd_nib_len = nibs.len() & 1 == 1;
    let prefix = ((is_odd_nib_len as u8) + ((is_leaf as u8) << 1)) << 4;
    let mut result = vec![prefix];
    for (i, nib) in nibs.iter().enumerate() {
        let is_odd_nib_index = i & 1 == 1;
        if is_odd_nib_len ^ is_odd_nib_index {
            // append to last byte
            *result.last_mut().unwrap() |= nib;
        } else {
            // append new byte
            result.push(nib << 4);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    #[test]
    pub fn test_trie_pointer_no_keccak() {
        let cases = [
            ("do", "verb"),
            ("dog", "puppy"),
            ("doge", "coin"),
            ("horse", "stallion"),
        ];
        for (k, v) in cases {
            let node: MptNode =
                MptNodeData::Leaf(k.as_bytes().to_vec(), v.as_bytes().to_vec()).into();
            assert!(
                matches!(node.pointer(),MptNodeReference::Bytes(bytes) if bytes == node.to_rlp().to_vec())
            );
        }
    }

    #[test]
    pub fn test_lcp() {
        let cases = [
            (vec![0xa, 0xb], vec![0xa, 0xc], 1),
            (vec![0xa, 0xb], vec![0xa, 0xb], 2),
            (vec![0xa, 0xb], vec![0xa, 0xb, 0xc], 2),
            (vec![0xa, 0xb, 0xc], vec![0xa, 0xb, 0xc], 3),
            (vec![0xa, 0xb, 0xc, 0xd], vec![0xa, 0xb, 0xc, 0xd], 4),
        ];
        for (a, b, cpl) in cases {
            assert_eq!(lcp(&a, &b), cpl)
        }
    }

    #[test]
    pub fn test_empty() {
        let trie = MptNode::default();
        let expected = hex!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421");
        assert_eq!(expected, trie.hash().0);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_tiny() {
        let mut trie = MptNode::default();
        trie.update(b"dog", b"puppy".to_vec());

        let expected = hex!("ed6e08740e4a267eca9d4740f71f573e9aabbcc739b16a2fa6c1baed5ec21278");
        assert_eq!(expected, trie.hash().0);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_update_branch_value() {
        let mut trie = MptNode::default();
        let vals = [("do", "verb"), ("dog", "puppy")];
        for (key, value) in &vals {
            trie.update(key.as_bytes(), value.as_bytes().to_vec());
        }

        let expected = hex!("779db3986dd4f38416bfde49750ef7b13c6ecb3e2221620bcad9267e94604d36");
        assert_eq!(expected, trie.hash().0);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_update() {
        let mut trie = MptNode::default();
        let vals = vec![
            ("doe", "reindeer"),
            ("dog", "puppy"),
            ("dogglesworth", "cat"),
        ];
        for (key, value) in &vals {
            trie.update(key.as_bytes(), value.as_bytes().to_vec());
        }
        let expected = hex!("8aad789dff2f538bca5d8ea56e8abe10f4c7ba3a5dea95fea4cd6e7c3a1168d3");
        assert_eq!(expected, trie.hash().0);

        for (key, value) in &vals {
            assert_eq!(trie.lookup(key.as_bytes()), Some(value.as_bytes()));
        }

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_delete() {
        let mut trie = MptNode::default();
        let vals = vec![
            ("do", "verb"),
            ("ether", "wookiedoo"),
            ("horse", "stallion"),
            ("shaman", "horse"),
            ("doge", "coin"),
            ("ether", ""),
            ("dog", "puppy"),
            ("shaman", ""),
        ];
        for (key, value) in vals {
            if value.is_empty() {
                trie.delete(key.as_bytes());
            } else {
                trie.update(key.as_bytes(), value.as_bytes().to_vec());
            }
        }

        let expected = hex!("5991bb8c6514148a29db676a14ac506cd2cd5775ace63c30a4fe457715e9ac84");
        assert_eq!(expected, trie.hash().0);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }
}
