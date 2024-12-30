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

use crate::data::{ArchivedMptNodeData, MptNodeData};
use crate::node::{ArchivedMptNode, MptNode};
use crate::pointer::MptNodePointer;
use crate::reference::{ArchivedMptNodeReference, MptNodeReference};
use crate::util;
use crate::util::{prefix_nibs, Error};
use alloy_primitives::bytes::BufMut;
use alloy_primitives::{keccak256, B256};
use alloy_rlp::{Decodable, Encodable};
use anyhow::{bail, Context};
use rkyv::option::ArchivedOption;

impl<'a> ArchivedMptNode<'a> {
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, Error> {
        self.data.get(&util::to_nibs(key))
    }

    #[inline]
    pub fn get_rlp<T: Decodable>(&self, key: &[u8]) -> Result<Option<T>, Error> {
        match self.get(key)? {
            Some(mut bytes) => Ok(Some(T::decode(&mut bytes)?)),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn delete(&'a self, key: &[u8]) -> Result<Option<MptNode>, Error> {
        let Some(replacement) = self.data.delete(&util::to_nibs(key))? else {
            return Ok(None);
        };
        let node = MptNode::from(replacement);

        Ok(Some(node))
    }

    #[inline]
    pub fn insert(&'a self, key: &[u8], value: Vec<u8>) -> Result<Option<MptNode>, Error> {
        if value.is_empty() {
            panic!("value must not be empty");
        }

        let Some(replacement) = self.data.insert(&util::to_nibs(key), value)? else {
            return Ok(None);
        };

        let node = MptNode::from(replacement);

        Ok(Some(node))
    }

    #[inline]
    pub fn insert_rlp(
        &'a mut self,
        key: &[u8],
        value: impl Encodable,
    ) -> Result<Option<MptNode>, Error> {
        self.insert(key, alloy_rlp::encode(value))
    }

    pub fn payload_length(&self) -> usize {
        match &self.data {
            ArchivedMptNodeData::Null => 0,
            ArchivedMptNodeData::Branch(nodes) => {
                1 + nodes
                    .iter()
                    .map(|child| child.as_ref().map_or(1, |node| node.reference_length()))
                    .sum::<usize>()
            }
            ArchivedMptNodeData::Leaf(prefix, value) => {
                prefix.as_slice().length() + value.as_slice().length()
            }
            ArchivedMptNodeData::Extension(prefix, node) => {
                prefix.as_slice().length() + node.reference_length()
            }
            ArchivedMptNodeData::Digest(_) => 32,
        }
    }

    pub fn reference_encode(&self, out: &mut dyn BufMut) {
        self.cached_reference.encode(out);
    }

    pub fn reference_length(&self) -> usize {
        self.cached_reference.len()
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.data, ArchivedMptNodeData::Null)
    }

    pub fn is_digest(&self) -> bool {
        matches!(self.data, ArchivedMptNodeData::Digest(_))
    }

    pub fn size(&self) -> usize {
        match &self.data {
            ArchivedMptNodeData::Null => 0,
            ArchivedMptNodeData::Branch(children) => {
                children.iter().flatten().map(|n| n.size()).sum::<usize>() + 1
            }
            ArchivedMptNodeData::Leaf(_, _) => 1,
            ArchivedMptNodeData::Extension(_, child) => child.size() + 1,
            ArchivedMptNodeData::Digest(_) => 0,
        }
    }

    pub fn verify_reference(&self) -> anyhow::Result<()> {
        match &self.data {
            ArchivedMptNodeData::Null => {
                if self.cached_reference.as_slice() != [alloy_rlp::EMPTY_STRING_CODE] {
                    bail!("Invalid empty node reference");
                }
            }
            ArchivedMptNodeData::Digest(digest) => {
                let Some(d) = self.cached_reference.as_digest() else {
                    bail!("Invalid digest node reference type");
                };
                if digest != d {
                    bail!("Invalid digest node reference");
                }
            }
            data => {
                // Verify children recursively and abort early
                match data {
                    ArchivedMptNodeData::Branch(children) => {
                        for c in children.iter().flatten() {
                            c.verify_reference()
                                .context("Invalid branch child reference")?;
                        }
                    }
                    ArchivedMptNodeData::Extension(_, child) => {
                        child
                            .verify_reference()
                            .context("Invalid extension node reference")?;
                    }
                    _ => {}
                }
                // Verify own encoding
                if MptNodeReference::from(alloy_rlp::encode(self)).as_slice()
                    != self.cached_reference.as_slice()
                {
                    bail!("Invalid node reference");
                }
            }
        }
        Ok(())
    }
}

impl<'a> ArchivedMptNodeData<'a> {
    pub fn get(&self, key_nibs: &[u8]) -> Result<Option<&[u8]>, Error> {
        match self {
            ArchivedMptNodeData::Null => Ok(None),
            ArchivedMptNodeData::Branch(nodes) => {
                if let Some((i, tail)) = key_nibs.split_first() {
                    match nodes[*i as usize] {
                        ArchivedOption::Some(ref node) => node.data.get(tail),
                        ArchivedOption::None => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            ArchivedMptNodeData::Leaf(prefix, value) => {
                if prefix_nibs(prefix) == key_nibs {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
            ArchivedMptNodeData::Extension(prefix, node) => {
                if let Some(tail) = key_nibs.strip_prefix(prefix_nibs(prefix).as_slice()) {
                    node.data.get(tail)
                } else {
                    Ok(None)
                }
            }
            ArchivedMptNodeData::Digest(digest) => Err(Error::NodeNotResolved(digest.0.into())),
        }
    }

    pub fn insert(&'a self, key_nibs: &[u8], value: Vec<u8>) -> Result<Option<MptNodeData>, Error> {
        match self {
            ArchivedMptNodeData::Null => Ok(Some(MptNodeData::Leaf(
                util::to_encoded_path(key_nibs, true),
                value,
            ))),
            ArchivedMptNodeData::Branch(children) => {
                let Some((i, tail)) = key_nibs.split_first() else {
                    return Err(Error::ValueInBranch);
                };

                let replacement = match children[*i as usize].as_ref() {
                    Some(node) => node.data.insert(tail, value)?,
                    None => {
                        // if the corresponding child is empty, insert a new leaf
                        Some(MptNodeData::Leaf(util::to_encoded_path(tail, true), value))
                    }
                };

                let Some(replacement) = replacement else {
                    return Ok(None);
                };

                let mut new_children: [Option<Box<MptNodePointer>>; 16] = Default::default();
                for (j, c) in children.iter().enumerate() {
                    if let ArchivedOption::Some(c) = c {
                        new_children[j] = Some(Box::new(MptNodePointer::Ref(c.as_ref())))
                    }
                }
                new_children[*i as usize] = Some(Box::new(replacement.into()));

                Ok(Some(MptNodeData::Branch(new_children)))
            }
            ArchivedMptNodeData::Leaf(prefix, old_value) => {
                let self_nibs = prefix_nibs(prefix);
                let common_len = util::lcp(&self_nibs, key_nibs);
                if common_len == self_nibs.len() && common_len == key_nibs.len() {
                    // if self_nibs == key_nibs, update the value if it is different
                    if old_value == &value {
                        Ok(None)
                    } else {
                        Ok(Some(MptNodeData::Leaf(prefix.to_vec(), value)))
                    }
                } else if common_len == self_nibs.len() || common_len == key_nibs.len() {
                    Err(Error::ValueInBranch)
                } else {
                    let split_point = common_len + 1;
                    // otherwise, create a branch with two children
                    let mut children: [Option<Box<MptNodePointer>>; 16] = Default::default();

                    children[self_nibs[common_len] as usize] = Some(Box::new(
                        MptNodeData::Leaf(
                            util::to_encoded_path(&self_nibs[split_point..], true),
                            old_value.to_vec(),
                        )
                        .into(),
                    ));
                    children[key_nibs[common_len] as usize] = Some(Box::new(
                        MptNodeData::Leaf(
                            util::to_encoded_path(&key_nibs[split_point..], true),
                            value,
                        )
                        .into(),
                    ));

                    let branch = MptNodeData::Branch(children);
                    if common_len > 0 {
                        // create parent extension for new branch
                        Ok(Some(MptNodeData::Extension(
                            util::to_encoded_path(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        )))
                    } else {
                        Ok(Some(branch))
                    }
                }
            }
            ArchivedMptNodeData::Extension(prefix, existing_child) => {
                let self_nibs = prefix_nibs(prefix);
                let common_len = util::lcp(&self_nibs, key_nibs);
                if common_len == self_nibs.len() {
                    // traverse down for update
                    let Some(new_child) =
                        existing_child.data.insert(&key_nibs[common_len..], value)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some(MptNodeData::Extension(
                        prefix.to_vec(),
                        Box::new(new_child.into()),
                    )))
                } else if common_len == key_nibs.len() {
                    Err(Error::ValueInBranch)
                } else {
                    let split_point = common_len + 1;
                    // otherwise, create a branch with two children
                    let mut children: [Option<Box<MptNodePointer>>; 16] = Default::default();

                    let existing_child = Box::new(existing_child.as_ref().into());
                    children[self_nibs[common_len] as usize] = if split_point < self_nibs.len() {
                        Some(Box::new(
                            MptNodeData::Extension(
                                util::to_encoded_path(&self_nibs[split_point..], false),
                                existing_child,
                            )
                            .into(),
                        ))
                    } else {
                        Some(existing_child)
                    };
                    children[key_nibs[common_len] as usize] = Some(Box::new(
                        MptNodeData::Leaf(
                            util::to_encoded_path(&key_nibs[split_point..], true),
                            value,
                        )
                        .into(),
                    ));

                    let branch = MptNodeData::Branch(children);
                    if common_len > 0 {
                        // Create parent extension for new branch
                        Ok(Some(MptNodeData::Extension(
                            util::to_encoded_path(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        )))
                    } else {
                        Ok(Some(branch))
                    }
                }
            }
            ArchivedMptNodeData::Digest(digest) => {
                Err(Error::NodeNotResolved(B256::from(digest.0)))
            }
        }
    }

    pub fn delete(&'a self, key_nibs: &[u8]) -> Result<Option<MptNodeData>, Error> {
        match self {
            ArchivedMptNodeData::Null => Ok(None),
            ArchivedMptNodeData::Branch(children) => {
                let Some((i, tail)) = key_nibs.split_first() else {
                    return Err(Error::ValueInBranch);
                };

                let Some(child) = children[*i as usize].as_ref() else {
                    return Ok(None);
                };

                let Some(replacement) = child.data.delete(tail)? else {
                    return Ok(None);
                };

                let mut new_children: [Option<Box<MptNodePointer>>; 16] = Default::default();
                for (j, c) in children.iter().enumerate() {
                    if let ArchivedOption::Some(c) = c {
                        new_children[j] = Some(Box::new(MptNodePointer::Ref(c.as_ref())))
                    }
                }
                // set option to none and maybe collapse if new child is null
                if let MptNodeData::Null = replacement {
                    new_children[*i as usize] = None;
                    let mut branch = MptNodeData::Branch(new_children);
                    branch.maybe_collapse(None)?;
                    Ok(Some(branch))
                } else {
                    new_children[*i as usize] = Some(Box::new(replacement.into()));
                    Ok(Some(MptNodeData::Branch(new_children)))
                }
            }
            ArchivedMptNodeData::Leaf(prefix, _) => {
                if prefix_nibs(prefix) != key_nibs {
                    Ok(None)
                } else {
                    Ok(Some(MptNodeData::Null))
                }
            }
            ArchivedMptNodeData::Extension(prefix, child) => {
                let self_nibs = prefix_nibs(prefix);
                let Some(tail) = key_nibs.strip_prefix(self_nibs.as_slice()) else {
                    return Ok(None);
                };

                let Some(replacement) = child.data.delete(tail)? else {
                    return Ok(None);
                };

                let mut new_extension =
                    MptNodeData::Extension(prefix.to_vec(), Box::new(replacement.into()));

                // an extension can only point to a branch or a digest; since it's sub trie was
                // modified, we need to make sure that this property still holds
                new_extension.maybe_collapse(Some(self_nibs))?;

                Ok(Some(new_extension))
            }
            ArchivedMptNodeData::Digest(digest) => Err(Error::NodeNotResolved(digest.0.into())),
        }
    }
}

impl Default for ArchivedMptNodeData<'_> {
    fn default() -> Self {
        Self::Null
    }
}

impl Encodable for ArchivedMptNode<'_> {
    #[inline]
    fn encode(&self, out: &mut dyn BufMut) {
        match &self.data {
            ArchivedMptNodeData::Null => {
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            }
            ArchivedMptNodeData::Branch(nodes) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                nodes.iter().for_each(|child| match child {
                    ArchivedOption::Some(node) => node.reference_encode(out),
                    ArchivedOption::None => out.put_u8(alloy_rlp::EMPTY_STRING_CODE),
                });
                // in the MPT reference, branches have values so always add empty value
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            }
            ArchivedMptNodeData::Leaf(prefix, value) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                prefix.as_slice().encode(out);
                value.as_slice().encode(out);
            }
            ArchivedMptNodeData::Extension(prefix, node) => {
                alloy_rlp::Header {
                    list: true,
                    payload_length: self.payload_length(),
                }
                .encode(out);
                prefix.as_slice().encode(out);
                node.reference_encode(out);
            }
            ArchivedMptNodeData::Digest(digest) => {
                digest.0.encode(out);
            }
        }
    }

    #[inline]
    fn length(&self) -> usize {
        let payload_length = self.payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

impl ArchivedMptNodeReference {
    pub fn is_digest(&self) -> bool {
        matches!(self, Self::Digest(_))
    }

    pub fn to_digest(&self) -> B256 {
        match self {
            ArchivedMptNodeReference::Bytes(b) => keccak256(b),
            ArchivedMptNodeReference::Digest(d) => d.0.into(),
        }
    }

    pub fn as_digest(&self) -> Option<&crate::util::ArchivedB256> {
        match self {
            ArchivedMptNodeReference::Bytes(_) => None,
            ArchivedMptNodeReference::Digest(d) => Some(d),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            ArchivedMptNodeReference::Bytes(b) => b.as_slice(),
            ArchivedMptNodeReference::Digest(d) => d.0.as_slice(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            ArchivedMptNodeReference::Bytes(b) => b.len(),
            ArchivedMptNodeReference::Digest(_) => 33, // length prefix + 32 bytes of data
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            ArchivedMptNodeReference::Bytes(b) => b.is_empty(),
            ArchivedMptNodeReference::Digest(_) => false,
        }
    }

    pub fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        if self.is_digest() {
            // if the reference is a digest, RLP-encode it with its fixed known length
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE + 32);
        }
        // if the reference is an RLP-encoded byte slice, copy it directly
        out.put_slice(self.as_slice());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keccak::keccak;

    #[test]
    pub fn test_read_write() {
        let mut owned = MptNodePointer::default();
        for i in 0..10000u128 {
            let data = i.to_be_bytes();
            owned
                .insert(&keccak(data.as_slice()), data.to_vec())
                .unwrap();
        }
        let encoded = rkyv::to_bytes::<rkyv::rancor::Error>(&owned).unwrap();
        let rkyved = rkyv::access::<ArchivedMptNode, rkyv::rancor::Error>(&encoded).unwrap();
        let mut ref_ptr = MptNodePointer::Ref(rkyved);
        assert_eq!(owned.hash(), ref_ptr.hash());
        for i in 10000..20000u128 {
            println!("insert {i}");
            let data = i.to_be_bytes();
            let key = keccak(data.as_slice());
            assert!(owned.insert(&key, data.to_vec()).unwrap());
            assert!(ref_ptr.insert(&key, data.to_vec()).unwrap());
            assert_eq!(owned.hash(), ref_ptr.hash());
        }
        for i in 0..20000u128 {
            println!("get {i}");
            let data = i.to_be_bytes();
            let key = keccak(data.as_slice());
            assert_eq!(owned.get(&key).unwrap().unwrap().to_vec(), data.to_vec());
            assert_eq!(ref_ptr.get(&key).unwrap().unwrap().to_vec(), data.to_vec());
        }
        for i in 0..20000u128 {
            println!("delete {i}");
            let data = i.to_be_bytes();
            let key = keccak(data.as_slice());
            assert!(owned.delete(&key).unwrap());
            assert!(ref_ptr.delete(&key).unwrap());
            assert_eq!(owned.hash(), ref_ptr.hash());
        }
        assert!(owned.is_empty());
        assert!(ref_ptr.is_empty());
    }
}
