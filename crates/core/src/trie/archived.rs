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

use crate::keccak::keccak;
use crate::trie::data::ArchivedMptNodeData;
use crate::trie::node::ArchivedMptNode;
use crate::trie::util::{prefix_nibs, to_nibs, Error};
use alloy_primitives::bytes::BufMut;
use alloy_rlp::{Decodable, Encodable};
use anyhow::{bail, Context};
use rkyv::option::ArchivedOption;

impl ArchivedMptNode {
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
        match &self.cached_reference {
            // if the reference is a digest, RLP-encode it with its fixed known length
            reference if reference.len() == 32 => {
                out.put_u8(alloy_rlp::EMPTY_STRING_CODE + 32);
                out.put_slice(reference.as_slice());
            }
            // if the reference is an RLP-encoded byte slice, copy it directly
            reference => out.put_slice(reference),
        }
    }

    pub fn reference_length(&self) -> usize {
        match &self.cached_reference {
            reference if reference.len() == 32 => 33,
            reference => reference.len(),
        }
    }

    pub fn verify_reference(&self) -> anyhow::Result<()> {
        match &self.data {
            ArchivedMptNodeData::Null => {
                if self.cached_reference.as_slice() != &[alloy_rlp::EMPTY_STRING_CODE] {
                    bail!("Invalid empty node reference");
                }
            }
            ArchivedMptNodeData::Digest(digest) => {
                if self.cached_reference.as_slice() != digest.0.as_slice() {
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
                let encoded = alloy_rlp::encode(self);
                if encoded.len() < 32 {
                    if self.cached_reference.as_slice() != encoded.as_slice() {
                        bail!("Invalid encoded node reference");
                    }
                } else {
                    if self.cached_reference.as_slice() != keccak(encoded).as_slice() {
                        bail!("Invalid digest reference");
                    }
                }
            }
        }
        Ok(())
    }

    #[inline]
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, Error> {
        self.data.get(&to_nibs(key))
    }

    #[inline]
    pub fn get_rlp<T: Decodable>(&self, key: &[u8]) -> Result<Option<T>, Error> {
        match self.get(key)? {
            Some(mut bytes) => Ok(Some(T::decode(&mut bytes)?)),
            None => Ok(None),
        }
    }
}

impl ArchivedMptNodeData {
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
}

impl Encodable for ArchivedMptNode {
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
