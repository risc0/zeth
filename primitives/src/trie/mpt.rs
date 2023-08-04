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
use rlp::{Decodable, DecoderError, Prototype, Rlp};
use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

use crate::{keccak::keccak, trie::EMPTY_ROOT, RlpBytes};

/// A node representing the root of a sparse Merkle Patricia trie. Sparse in this context
/// means that certain unneeded parts of the trie, i.e. sub-tries, can be cut off and
/// represented by their node hash. However, if a trie operation such as `insert`,
/// `remove` or `get` ends up in such a truncated part, it cannot be executed and returns
/// an error. Another difference from other Merkle Patricia trie implementations is that
/// branches cannot store values. Due to the way how MPTs are constructed in Ethereum,
/// this is never needed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct MptNode {
    /// The type and data of the node.
    data: MptNodeData,
    /// Cache for a previously computed [MptNodeReference] of this node.
    #[serde(skip)]
    cached_reference: RefCell<Option<MptNodeReference>>,
}

/// Merkle Patricia trie error type.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("reached an unresolved node: {0:#}")]
    NodeNotResolved(B256),
    #[error("branch node with value")]
    ValueInBranch,
    #[error("RLP error")]
    Rlp(#[from] alloy_rlp::Error),
    #[error("RLP error")]
    LegacyRlp(#[from] DecoderError),
}

/// The type and data of a node in a Merkle Patricia trie.
#[derive(Clone, Debug, Default, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MptNodeData {
    /// Empty trie node.
    #[default]
    Null,
    /// Node with at most 16 children.
    Branch([Box<MptNode>; 16]),
    /// Leaf node with a value.
    Leaf(Vec<u8>, Vec<u8>),
    /// Node with exactly one child.
    Extension(Vec<u8>, Box<MptNode>),
    /// Representation of a sub-trie by its hash.
    Digest(B256),
}

/// Reference of one node inside another node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MptNodeReference {
    /// Short encodings (less than 32 bytes).
    Bytes(Vec<u8>),
    /// Keccak hash of long encodings (not less than 32 bytes).
    Digest(B256),
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
            MptNodeData::Branch(nodes) => {
                let mut payload_length = 0;
                for node in nodes {
                    payload_length += node.reference_length();
                }
                payload_length += 1;
                alloy_rlp::Header {
                    list: true,
                    payload_length,
                }
                .encode(out);
                for node in nodes {
                    node.reference_encode(out);
                }
                // in the MPT reference, branches have values so always add empty value
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
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
                let payload_length = prefix.as_slice().length() + node.reference_length();
                alloy_rlp::Header {
                    list: true,
                    payload_length,
                }
                .encode(out);
                prefix.as_slice().encode(out);
                node.reference_encode(out);
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
                let value: Vec<u8> = rlp.val_at(16)?;
                if value.is_empty() {
                    Ok(MptNodeData::Branch(node_list.try_into().unwrap()).into())
                } else {
                    Err(DecoderError::Custom("branch node with value"))
                }
            }
            Prototype::Data(32) => {
                let bytes: Vec<u8> = rlp.as_val()?;
                Ok(MptNodeData::Digest(B256::from_slice(&bytes)).into())
            }
            _ => Err(DecoderError::RlpIncorrectListLen),
        }
    }
}

impl MptNode {
    /// Clears the trie, replacing it with [MptNodeData::Null].
    pub fn clear(&mut self) {
        self.data = MptNodeData::Null;
        self.invalidate_ref_cache();
    }

    /// Decodes an RLP-encoded [MptNode].
    pub fn decode(bytes: impl AsRef<[u8]>) -> Result<MptNode, Error> {
        rlp::decode(bytes.as_ref()).map_err(Error::from)
    }

    /// Returns the type and data of the node.
    pub fn as_data(&self) -> &MptNodeData {
        &self.data
    }

    /// Returns the 256-bit hash of the node.
    pub fn hash(&self) -> B256 {
        match self.data {
            MptNodeData::Null => EMPTY_ROOT,
            _ => match self.reference() {
                MptNodeReference::Digest(digest) => digest,
                MptNodeReference::Bytes(bytes) => keccak(bytes).into(),
            },
        }
    }

    /// Returns the [MptNodeReference] of this node when referenced inside another node.
    pub fn reference(&self) -> MptNodeReference {
        self.cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
            .clone()
    }

    /// Encodes the [MptNodeReference] of this node into the `out` buffer.
    fn reference_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
        {
            MptNodeReference::Bytes(bytes) => out.put_slice(bytes),
            MptNodeReference::Digest(digest) => digest.encode(out),
        }
    }

    /// Returns the length of the encoded [MptNodeReference] of this node.
    fn reference_length(&self) -> usize {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
        {
            MptNodeReference::Bytes(bytes) => bytes.len(),
            MptNodeReference::Digest(digest) => digest.length(),
        }
    }

    fn calc_reference(&self) -> MptNodeReference {
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
    pub fn is_empty(&self) -> bool {
        matches!(&self.data, MptNodeData::Null)
    }

    /// Returns whether the trie is resolved or is the reference of another node.
    pub fn is_resolved(&self) -> bool {
        !matches!(&self.data, MptNodeData::Digest(_))
    }

    /// Returns the nibbles corresponding to the node's prefix.
    pub fn nibs(&self) -> Vec<u8> {
        match &self.data {
            MptNodeData::Null | MptNodeData::Branch(_) | MptNodeData::Digest(_) => vec![],
            MptNodeData::Leaf(prefix, _) | MptNodeData::Extension(prefix, _) => {
                let extension = prefix[0];
                // the first bit of the first nibble denotes the parity
                let is_odd = extension & (1 << 4) != 0;

                let mut result = Vec::with_capacity(2 * prefix.len() - 1);
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

    /// Returns a reference to the value corresponding to the key.
    /// If [None] is returned, the key is provably not in the trie.
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, Error> {
        self.get_internal(&to_nibs(key))
    }

    /// Returns the RLP-decoded value corresponding to the key.
    /// If [None] is returned, the key is provably not in the trie.
    pub fn get_rlp<T: alloy_rlp::Decodable>(&self, key: &[u8]) -> Result<Option<T>, Error> {
        match self.get(key)? {
            Some(mut bytes) => Ok(Some(T::decode(&mut bytes)?)),
            None => Ok(None),
        }
    }

    fn get_internal(&self, key_nibs: &[u8]) -> Result<Option<&[u8]>, Error> {
        match &self.data {
            MptNodeData::Null => Ok(None),
            MptNodeData::Branch(nodes) => {
                if key_nibs.is_empty() {
                    Ok(None)
                } else {
                    nodes
                        .get(key_nibs[0] as usize)
                        .unwrap()
                        .get_internal(&key_nibs[1..])
                }
            }
            MptNodeData::Leaf(_, value) => {
                if self.nibs() == key_nibs {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
            MptNodeData::Extension(_, node) => {
                let ext_nibs = self.nibs();
                let ext_len = ext_nibs.len();
                if key_nibs[..ext_len] != ext_nibs {
                    Ok(None)
                } else {
                    node.get_internal(&key_nibs[ext_len..])
                }
            }
            MptNodeData::Digest(digest) => Err(Error::NodeNotResolved(*digest)),
        }
    }

    // Removes a key from the trie. It returns `true` when that key had a value associated
    // with it, or `false` if the key was provably not in the trie.
    pub fn delete(&mut self, key: &[u8]) -> Result<bool, Error> {
        self.delete_internal(&to_nibs(key))
    }

    fn delete_internal(&mut self, key_nibs: &[u8]) -> Result<bool, Error> {
        let mut self_nibs = self.nibs();
        match &mut self.data {
            MptNodeData::Null => return Ok(false),
            MptNodeData::Branch(children) => {
                if key_nibs.is_empty() {
                    return Ok(false);
                }
                let child = children.get_mut(key_nibs[0] as usize).unwrap();
                if !child.delete_internal(&key_nibs[1..])? {
                    return Ok(false);
                }

                let mut remaining = children
                    .iter_mut()
                    .enumerate()
                    .filter(|(_, n)| !n.is_empty());
                // there will always be at least one remaining node
                let (index, node) = remaining.next().unwrap();
                // if there is only exactly one node left, we need to convert the branch
                if remaining.next().is_none() {
                    let mut orphan = mem::take(node);

                    let orphan_nibs = orphan.nibs().into_iter();
                    match &mut orphan.data {
                        // if the orphan is a leaf, prepend the corresponding nib to it
                        MptNodeData::Leaf(_, orphan_value) => {
                            let new_nibs: Vec<_> =
                                iter::once(index as u8).chain(orphan_nibs).collect();
                            self.data = MptNodeData::Leaf(
                                to_prefix(&new_nibs, true),
                                mem::take(orphan_value),
                            );
                        }
                        // if the orphan is an extension, prepend the corresponding nib to it
                        MptNodeData::Extension(_, orphan_child) => {
                            let new_nibs: Vec<_> =
                                iter::once(index as u8).chain(orphan_nibs).collect();
                            self.data = MptNodeData::Extension(
                                to_prefix(&new_nibs, false),
                                mem::take(orphan_child),
                            );
                        }
                        // if the orphan is a branch or digest, convert to an extension
                        MptNodeData::Branch(_) | MptNodeData::Digest(_) => {
                            self.data =
                                MptNodeData::Extension(to_prefix(&[index as u8], false), orphan);
                        }
                        MptNodeData::Null => unreachable!(),
                    }
                }
            }
            MptNodeData::Leaf(_, _) => {
                if self_nibs != key_nibs {
                    return Ok(false);
                }
                self.data = MptNodeData::Null;
            }
            MptNodeData::Extension(_, child) => {
                let ext_len = self_nibs.len();
                if key_nibs[..ext_len] != self_nibs {
                    return Ok(false);
                }
                if !child.delete_internal(&key_nibs[ext_len..])? {
                    return Ok(false);
                }

                // an extension can only point to a branch or a digest
                // if this is no longer the case, it needs to be cleaned up
                let child_nibs = child.nibs().into_iter();
                match &mut child.data {
                    // if the extension points to nothing, it can be removed as well
                    MptNodeData::Null => {
                        self.data = MptNodeData::Null;
                    }
                    // if the extension points to a leaf, make the leaf longer
                    MptNodeData::Leaf(_, value) => {
                        self_nibs.extend(child_nibs);
                        self.data =
                            MptNodeData::Leaf(to_prefix(&self_nibs, true), mem::take(value));
                    }
                    // if the extension points to an extension, make the extension longer
                    MptNodeData::Extension(_, node) => {
                        self_nibs.extend(child_nibs);
                        self.data =
                            MptNodeData::Extension(to_prefix(&self_nibs, false), mem::take(node));
                    }
                    MptNodeData::Branch(_) | MptNodeData::Digest(_) => {}
                }
            }
            MptNodeData::Digest(digest) => return Err(Error::NodeNotResolved(*digest)),
        };

        self.invalidate_ref_cache();
        Ok(true)
    }

    /// Inserts a key-value pair into the trie returning whether the trie has changed.
    pub fn insert(&mut self, key: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        if value.is_empty() {
            panic!("value must not be empty");
        }
        self.insert_internal(&to_nibs(key), value)
    }

    /// Inserts an RLP-encoded value into the trie returning whether the trie has changed.
    pub fn insert_rlp(&mut self, key: &[u8], value: impl Encodable) -> Result<bool, Error> {
        self.insert_internal(&to_nibs(key), value.to_rlp())
    }

    fn insert_internal(&mut self, key_nibs: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        let self_nibs = self.nibs();
        match &mut self.data {
            MptNodeData::Null => {
                self.data = MptNodeData::Leaf(to_prefix(key_nibs, true), value);
            }
            MptNodeData::Branch(children) => {
                if key_nibs.is_empty() {
                    return Err(Error::ValueInBranch);
                }
                let child = children.get_mut(key_nibs[0] as usize).unwrap();
                if !child.insert_internal(&key_nibs[1..], value)? {
                    return Ok(false);
                }
            }
            MptNodeData::Leaf(_, old_value) => {
                let common_len = lcp(&self_nibs, key_nibs);
                if common_len == self_nibs.len() && common_len == key_nibs.len() {
                    // if self_nibs == key_nibs, update the value if it is different
                    if old_value == &value {
                        return Ok(false);
                    }
                    *old_value = value;
                } else if common_len == self_nibs.len() || common_len == key_nibs.len() {
                    return Err(Error::ValueInBranch);
                } else {
                    let split_point = common_len + 1;
                    // otherwise, create a branch with two children
                    let mut children: [Box<MptNode>; 16] = Default::default();

                    children[self_nibs[common_len] as usize] = Box::new(
                        MptNodeData::Leaf(
                            to_prefix(&self_nibs[split_point..], true),
                            mem::take(old_value),
                        )
                        .into(),
                    );
                    children[key_nibs[common_len] as usize] = Box::new(
                        MptNodeData::Leaf(to_prefix(&key_nibs[split_point..], true), value).into(),
                    );

                    let branch = MptNodeData::Branch(children);
                    if common_len > 0 {
                        // create parent extension for new branch
                        self.data = MptNodeData::Extension(
                            to_prefix(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        self.data = branch;
                    }
                }
            }
            MptNodeData::Extension(_, existing_child) => {
                let common_len = lcp(&self_nibs, key_nibs);
                if common_len == self_nibs.len() {
                    // traverse down for update
                    if !existing_child.insert_internal(&key_nibs[common_len..], value)? {
                        return Ok(false);
                    }
                } else if common_len == key_nibs.len() {
                    return Err(Error::ValueInBranch);
                } else {
                    let split_point = common_len + 1;
                    // otherwise, create a branch with two children
                    let mut children: [Box<MptNode>; 16] = Default::default();

                    children[self_nibs[common_len] as usize] = if split_point < self_nibs.len() {
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
                    children[key_nibs[common_len] as usize] = Box::new(
                        MptNodeData::Leaf(to_prefix(&key_nibs[split_point..], true), value).into(),
                    );

                    let branch = MptNodeData::Branch(children);
                    if common_len > 0 {
                        // Create parent extension for new branch
                        self.data = MptNodeData::Extension(
                            to_prefix(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        self.data = branch;
                    }
                }
            }
            MptNodeData::Digest(digest) => return Err(Error::NodeNotResolved(*digest)),
        };

        self.invalidate_ref_cache();
        Ok(true)
    }

    fn invalidate_ref_cache(&mut self) {
        self.cached_reference.borrow_mut().take();
    }

    /// Formats the trie as string list, where each line corresponds to a trie leaf.
    pub fn debug_rlp<T: alloy_rlp::Decodable + Debug>(&self) -> Vec<String> {
        let nibs: String = self.nibs().iter().map(|n| format!("{:x}", n)).collect();
        match self.as_data() {
            MptNodeData::Null => vec![format!("{:?}", MptNodeData::Null)],
            MptNodeData::Branch(nodes) => nodes
                .iter()
                .enumerate()
                .flat_map(|(i, n)| {
                    n.debug_rlp::<T>()
                        .into_iter()
                        .map(move |s| format!("{:x} {}", i, s))
                })
                .collect(),
            MptNodeData::Leaf(_, data) => {
                vec![format!(
                    "{} -> [{:?}]",
                    nibs,
                    T::decode(&mut &data[..]).unwrap()
                )]
            }
            MptNodeData::Extension(_, node) => node
                .debug_rlp::<T>()
                .into_iter()
                .map(|s| format!("{} {}", nibs, s))
                .collect(),
            MptNodeData::Digest(digest) => vec![format!("#{:#}", digest)],
        }
    }
}

/// Converts a byte slice to nibs.
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

/// Returns the length of the common prefix.
fn lcp(a: &[u8], b: &[u8]) -> usize {
    let mut a = a.iter();
    let mut b = b.iter();
    let mut res = 0;
    loop {
        match (a.next(), b.next()) {
            (Some(a), Some(b)) => {
                if a != b {
                    return res;
                }
            }
            _ => return res,
        }
        res += 1
    }
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
                matches!(node.reference(),MptNodeReference::Bytes(bytes) if bytes == node.to_rlp().to_vec())
            );
        }
    }

    #[test]
    pub fn test_lcp() {
        let cases = [
            (vec![], vec![], 0),
            (vec![0xa], vec![0xa], 1),
            (vec![0xa, 0xb], vec![0xa, 0xc], 1),
            (vec![0xa, 0xb], vec![0xa, 0xb], 2),
            (vec![0xa, 0xb], vec![0xa, 0xb, 0xc], 2),
            (vec![0xa, 0xb, 0xc], vec![0xa, 0xb, 0xc], 3),
            (vec![0xa, 0xb, 0xc], vec![0xa, 0xb, 0xc, 0xd], 3),
            (vec![0xa, 0xb, 0xc, 0xd], vec![0xa, 0xb, 0xc, 0xd], 4),
        ];
        for (a, b, cpl) in cases {
            assert_eq!(lcp(&a, &b), cpl)
        }
    }

    #[test]
    pub fn test_empty() {
        let trie = MptNode::default();

        assert!(trie.is_empty());
        assert_eq!(trie.reference(), MptNodeReference::Bytes(vec![0x80]));
        let expected = hex!("56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421");
        assert_eq!(expected, trie.hash().0);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_tiny() {
        // trie consisting of an extension, a branch and two leafs
        let mut trie = MptNode::default();
        trie.insert_rlp(b"a", 0u8).unwrap();
        trie.insert_rlp(b"b", 1u8).unwrap();

        assert!(!trie.is_empty());
        let expected = hex!("d816d680c3208180c220018080808080808080808080808080");
        assert_eq!(trie.reference(), MptNodeReference::Bytes(expected.to_vec()));
        let expected = hex!("6fbf23d6ec055dd143ff50d558559770005ff44ae1d41276f1bd83affab6dd3b");
        assert_eq!(trie.hash().0, expected);

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_branch_value() {
        let mut trie = MptNode::default();
        trie.insert(b"do", b"verb".to_vec()).unwrap();
        // leads to a branch with value which is not supported
        trie.insert(b"dog", b"puppy".to_vec()).unwrap_err();
    }

    #[test]
    pub fn test_update() {
        let mut trie = MptNode::default();
        let vals = vec![
            ("painting", "place"),
            ("guest", "ship"),
            ("mud", "leave"),
            ("paper", "call"),
            ("gate", "boast"),
            ("tongue", "gain"),
            ("baseball", "wait"),
            ("tale", "lie"),
            ("mood", "cope"),
            ("menu", "fear"),
        ];
        for i in 0..vals.len() {
            let (key, val) = vals[i];
            trie.insert(key.as_bytes(), val.as_bytes().to_vec())
                .unwrap();
        }

        let expected = hex!("2bab6cdf91a23ebf3af683728ea02403a98346f99ed668eec572d55c70a4b08f");
        assert_eq!(expected, trie.hash().0);

        for (key, value) in &vals {
            assert_eq!(trie.get(key.as_bytes()).unwrap(), Some(value.as_bytes()));
        }

        // try RLP roundtrip
        let decoded = MptNode::decode(trie.to_rlp()).unwrap();
        assert_eq!(trie.hash(), decoded.hash());
    }

    #[test]
    pub fn test_keccak_trie() {
        const N: usize = 512;

        // insert
        let mut trie = MptNode::default();
        for i in 0..N {
            assert!(trie.insert_rlp(&keccak(i.to_be_bytes()), i).unwrap());

            // check hash against trie build in reverse
            let mut reference = MptNode::default();
            for j in (0..=i).rev() {
                reference.insert_rlp(&keccak(j.to_be_bytes()), j).unwrap();
            }
            assert_eq!(trie.hash(), reference.hash());
        }

        let expected = hex!("7310027edebdd1f7c950a7fb3413d551e85dff150d45aca4198c2f6315f9b4a7");
        assert_eq!(trie.hash().0, expected);

        // get
        for i in 0..N {
            assert_eq!(trie.get_rlp(&keccak(i.to_be_bytes())).unwrap(), Some(i));
            assert!(trie.get(&keccak((i + N).to_be_bytes())).unwrap().is_none());
        }

        // delete
        for i in 0..N {
            assert!(trie.delete(&keccak(i.to_be_bytes())).unwrap());

            let mut reference = MptNode::default();
            for j in ((i + 1)..N).rev() {
                reference.insert_rlp(&keccak(j.to_be_bytes()), j).unwrap();
            }
            assert_eq!(trie.hash(), reference.hash());
        }
        assert!(trie.is_empty());
    }

    #[test]
    pub fn test_index_trie() {
        const N: usize = 512;

        // insert
        let mut trie = MptNode::default();
        for i in 0..N {
            assert!(trie.insert_rlp(&i.to_rlp(), i).unwrap());

            // check hash against trie build in reverse
            let mut reference = MptNode::default();
            for j in (0..=i).rev() {
                reference.insert_rlp(&j.to_rlp(), j).unwrap();
            }
            assert_eq!(trie.hash(), reference.hash());

            // try RLP roundtrip
            let decoded = MptNode::decode(trie.to_rlp()).unwrap();
            assert_eq!(trie.hash(), decoded.hash());
        }

        // get
        for i in 0..N {
            assert_eq!(trie.get_rlp(&i.to_rlp()).unwrap(), Some(i));
            assert!(trie.get(&(i + N).to_rlp()).unwrap().is_none());
        }

        // delete
        for i in 0..N {
            assert!(trie.delete(&i.to_rlp()).unwrap());

            let mut reference = MptNode::default();
            for j in ((i + 1)..N).rev() {
                reference.insert_rlp(&j.to_rlp(), j).unwrap();
            }
            assert_eq!(trie.hash(), reference.hash());
        }
        assert!(trie.is_empty());
    }
}
