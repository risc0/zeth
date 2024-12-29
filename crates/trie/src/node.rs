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
use crate::keccak::keccak;
use crate::reference::{CachedMptRef, MptNodeReference};
use crate::util;
use crate::util::Error;
use alloy_consensus::EMPTY_ROOT_HASH;
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable};
use arrayvec::ArrayVec;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fmt::{Debug, Write};

/// Represents the root node of a sparse Merkle Patricia Trie.
///
/// The "sparse" nature of this trie allows for truncation of certain unneeded parts,
/// representing them by their node hash. This design choice is particularly useful for
/// optimizing storage. However, operations targeting a truncated part will fail and
/// return an error. Another distinction of this implementation is that branches cannot
/// store values, aligning with the construction of MPTs in Ethereum.
#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[rkyv(bytecheck(
    bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source,
    )
))]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(
    __D::Error: rkyv::rancor::Source
))]
#[rkyv(derive(Debug, Eq, PartialEq))]
pub struct MptNode<'a> {
    /// The type and data of the node.
    #[rkyv(omit_bounds)]
    pub data: MptNodeData<'a>,
    /// Cache for a previously computed reference of this node. This is skipped during
    /// serialization.
    #[serde(skip)]
    #[rkyv(with = crate::reference::RequireCachedRef)]
    pub cached_reference: CachedMptRef,
}

impl From<B256> for MptNode<'_> {
    fn from(digest: B256) -> Self {
        match digest {
            EMPTY_ROOT_HASH | B256::ZERO => MptNode::default(),
            _ => MptNodeData::Digest(digest).into(),
        }
    }
}

/// Provides a conversion from [MptNodeData] to [MptNode].
///
/// This implementation allows for conversion from [MptNodeData] to [MptNode],
/// initializing the `data` field with the provided value and setting the
/// `cached_reference` field to `None`.
impl<'a> From<MptNodeData<'a>> for MptNode<'a> {
    fn from(value: MptNodeData<'a>) -> Self {
        Self {
            data: value,
            cached_reference: RefCell::new(None),
        }
    }
}

/// Represents a node in the sparse Merkle Patricia Trie (MPT).
///
/// The [MptNode] type encapsulates the data and functionalities associated with a node in
/// the MPT. It provides methods for manipulating the trie, such as inserting, deleting,
/// and retrieving values, as well as utility methods for encoding, decoding, and
/// debugging.
impl<'a> MptNode<'a> {
    /// Clears the trie, replacing its data with an empty node, [MptNodeData::Null].
    ///
    /// This method effectively removes all key-value pairs from the trie.
    #[inline]
    pub fn clear(&mut self) {
        self.data = MptNodeData::Null;
        self.invalidate_ref_cache();
    }

    /// Retrieves the underlying data of the node.
    ///
    /// This method provides a reference to the node's data, allowing for inspection and
    /// manipulation.
    #[inline]
    pub fn as_data(&self) -> &MptNodeData<'a> {
        &self.data
    }

    /// Retrieves the underlying data of the node.
    ///
    /// This method provides a reference to the node's data, allowing for inspection and
    /// manipulation.
    #[inline]
    pub fn as_data_mut(&mut self) -> &mut MptNodeData<'a> {
        &mut self.data
    }

    #[inline]
    pub fn is_reference_cached(&self) -> bool {
        self.cached_reference.borrow().is_some()
    }

    /// Retrieves the [MptNodeReference] reference of the node when it's referenced inside
    /// another node.
    ///
    /// This method provides a way to obtain a compact representation of the node for
    /// storage or transmission purposes.
    #[inline]
    pub fn reference(&self) -> MptNodeReference {
        self.cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
            .clone()
    }

    /// Computes and returns the 256-bit hash of the node.
    ///
    /// This method provides a unique identifier for the node based on its content.
    #[inline]
    pub fn hash(&self) -> B256 {
        match self.data {
            MptNodeData::Null => EMPTY_ROOT_HASH,
            _ => match self
                .cached_reference
                .borrow_mut()
                .get_or_insert_with(|| self.calc_reference())
            {
                reference if reference.is_full() => B256::from_slice(reference.as_slice()),
                reference => keccak(reference.as_slice()).into(),
            },
        }
    }

    /// Encodes the [MptNodeReference] of this node into the `out` buffer.
    pub fn reference_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
        {
            // if the reference is a digest, RLP-encode it with its fixed known length
            reference if reference.is_full() => {
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE + 32);
                out.put_slice(reference.as_slice());
            }
            // if the reference is an RLP-encoded byte slice, copy it directly
            reference => out.put_slice(reference),
        }
    }

    /// Returns the length of the encoded [MptNodeReference] of this node.
    pub fn reference_length(&self) -> usize {
        match self
            .cached_reference
            .borrow_mut()
            .get_or_insert_with(|| self.calc_reference())
        {
            reference if reference.is_full() => 33,
            reference => reference.len(),
        }
    }

    pub fn calc_reference(&self) -> MptNodeReference {
        match &self.data {
            MptNodeData::Null => {
                let mut encoded = ArrayVec::new();
                encoded.push(alloy_rlp::EMPTY_STRING_CODE);
                encoded
            }
            MptNodeData::Digest(digest) => digest.0.into(),
            _ => {
                let encoded = alloy_rlp::encode(self);
                if encoded.len() < 32 {
                    ArrayVec::from_iter(encoded)
                } else {
                    keccak(encoded).into()
                }
            }
        }
    }

    /// Determines if the trie is empty.
    ///
    /// This method checks if the node represents an empty trie, i.e., it doesn't contain
    /// any key-value pairs.
    #[inline]
    pub fn is_empty(&self) -> bool {
        matches!(&self.data, MptNodeData::Null)
    }

    /// Determines if the node represents a digest.
    ///
    /// A digest is a compact representation of a sub-trie, represented by its hash.
    #[inline]
    pub fn is_digest(&self) -> bool {
        matches!(&self.data, MptNodeData::Digest(_))
    }

    /// Retrieves the nibbles corresponding to the node's prefix.
    ///
    /// Nibbles are half-bytes, and in the context of the MPT, they represent parts of
    /// keys.
    #[inline]
    pub fn nibs(&self) -> Vec<u8> {
        match &self.data {
            MptNodeData::Null | MptNodeData::Branch(_) | MptNodeData::Digest(_) => vec![],
            MptNodeData::Leaf(prefix, _) | MptNodeData::Extension(prefix, _) => {
                util::prefix_nibs(prefix)
            }
        }
    }

    /// Retrieves the value associated with a given key in the trie.
    ///
    /// If the key is not present in the trie, this method returns `None`. Otherwise, it
    /// returns a reference to the associated value. If [None] is returned, the key is
    /// provably not in the trie.
    #[inline]
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, Error> {
        self.data.get(&util::to_nibs(key))
    }

    /// Retrieves the RLP-decoded value corresponding to the key.
    ///
    /// If the key is not present in the trie, this method returns `None`. Otherwise, it
    /// returns the RLP-decoded value.
    #[inline]
    pub fn get_rlp<T: Decodable>(&self, key: &[u8]) -> Result<Option<T>, Error> {
        match self.get(key)? {
            Some(mut bytes) => Ok(Some(T::decode(&mut bytes)?)),
            None => Ok(None),
        }
    }

    /// Removes a key from the trie.
    ///
    /// This method attempts to remove a key-value pair from the trie. If the key is
    /// present, it returns `true`. Otherwise, it returns `false`.
    #[inline]
    pub fn delete(&mut self, key: &[u8]) -> Result<bool, Error> {
        if self.data.delete(&util::to_nibs(key))? {
            self.invalidate_ref_cache();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Inserts a key-value pair into the trie.
    ///
    /// This method attempts to insert a new key-value pair into the trie. If the
    /// insertion is successful, it returns `true`. If the key already exists, it updates
    /// the value and returns `false`.
    #[inline]
    pub fn insert(&mut self, key: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        if value.is_empty() {
            panic!("value must not be empty");
        }
        if self.data.insert(&util::to_nibs(key), value)? {
            self.invalidate_ref_cache();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Inserts an RLP-encoded value into the trie.
    ///
    /// This method inserts a value that's been encoded using RLP into the trie.
    #[inline]
    pub fn insert_rlp(&mut self, key: &[u8], value: impl Encodable) -> Result<bool, Error> {
        self.insert(key, alloy_rlp::encode(value))
    }

    pub fn invalidate_ref_cache(&mut self) {
        self.cached_reference.borrow_mut().take();
    }

    /// Returns the number of traversable nodes in the trie.
    ///
    /// This method provides a count of all the nodes that can be traversed within the
    /// trie.
    pub fn size(&self) -> usize {
        match self.as_data() {
            MptNodeData::Null => 0,
            MptNodeData::Branch(children) => {
                children.iter().flatten().map(|n| n.size()).sum::<usize>() + 1
            }
            MptNodeData::Leaf(_, _) => 1,
            MptNodeData::Extension(_, child) => child.size() + 1,
            MptNodeData::Digest(_) => 0,
        }
    }

    /// Formats the trie as a string list, where each line corresponds to a trie leaf.
    ///
    /// This method is primarily used for debugging purposes, providing a visual
    /// representation of the trie's structure.
    pub fn debug_rlp<T: alloy_rlp::Decodable + Debug>(&self) -> Vec<String> {
        // convert the nibs to hex
        let nibs: String = self.nibs().iter().fold(String::new(), |mut output, n| {
            let _ = write!(output, "{:x}", n);
            output
        });

        match self.as_data() {
            MptNodeData::Null => vec![String::from("MptNodeData::Null")],
            MptNodeData::Branch(children) => children
                .iter()
                .enumerate()
                .flat_map(|(i, child)| {
                    match child {
                        Some(node) => node.debug_rlp::<T>(),
                        None => vec!["None".to_string()],
                    }
                    .into_iter()
                    .map(move |s| format!("{:x} {}", i, s))
                })
                .collect(),
            MptNodeData::Leaf(_, data) => {
                vec![format!(
                    "{} -> {:?}",
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

    /// Returns the length of the RLP payload of the node.
    pub fn payload_length(&self) -> usize {
        match &self.data {
            MptNodeData::Null => 0,
            MptNodeData::Branch(nodes) => {
                1 + nodes
                    .iter()
                    .map(|child| child.as_ref().map_or(1, |node| node.reference_length()))
                    .sum::<usize>()
            }
            MptNodeData::Leaf(prefix, value) => {
                prefix.as_slice().length() + value.as_slice().length()
            }
            MptNodeData::Extension(prefix, node) => {
                prefix.as_slice().length() + node.reference_length()
            }
            MptNodeData::Digest(_) => 32,
        }
    }
}
