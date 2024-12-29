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

use crate::pointer::MptNodePointer;
use crate::util;
use crate::util::Error;
use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use std::{iter, mem};

/// Represents the various types of data that can be stored within a node in the sparse
/// Merkle Patricia Trie (MPT).
///
/// Each node in the trie can be of one of several types, each with its own specific data
/// structure. This enum provides a clear and type-safe way to represent the data
/// associated with each node type.
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
pub enum MptNodeData<'a> {
    /// Represents an empty trie node.
    #[default]
    Null,
    /// A node that can have up to 16 children. Each child is an optional boxed [MptNode].
    Branch(#[rkyv(omit_bounds)] [Option<Box<MptNodePointer<'a>>>; 16]),
    /// A leaf node that contains a key and a value, both represented as byte vectors.
    Leaf(Vec<u8>, Vec<u8>),
    /// A node that has exactly one child and is used to represent a shared prefix of
    /// several keys.
    Extension(Vec<u8>, #[rkyv(omit_bounds)] Box<MptNodePointer<'a>>),
    /// Represents a sub-trie by its hash, allowing for efficient storage of large
    /// sub-tries without storing their entire content.
    Digest(#[rkyv(with = crate::util::B256Def)] B256),
}

impl<'a> MptNodeData<'a> {
    pub fn get(&self, key_nibs: &[u8]) -> Result<Option<&[u8]>, Error> {
        match &self {
            MptNodeData::Null => Ok(None),
            MptNodeData::Branch(nodes) => {
                if let Some((i, tail)) = key_nibs.split_first() {
                    match nodes[*i as usize] {
                        Some(ref node) => node.get_data(tail),
                        None => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            MptNodeData::Leaf(prefix, value) => {
                if util::prefix_nibs(prefix) == key_nibs {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
            MptNodeData::Extension(prefix, node) => {
                if let Some(tail) = key_nibs.strip_prefix(util::prefix_nibs(prefix).as_slice()) {
                    node.get_data(tail)
                } else {
                    Ok(None)
                }
            }
            MptNodeData::Digest(digest) => Err(Error::NodeNotResolved(*digest)),
        }
    }

    pub fn insert(&mut self, key_nibs: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        match self {
            MptNodeData::Null => {
                *self = MptNodeData::Leaf(util::to_encoded_path(key_nibs, true), value);
            }
            MptNodeData::Branch(children) => {
                if let Some((i, tail)) = key_nibs.split_first() {
                    let child = &mut children[*i as usize];
                    match child {
                        Some(node) => {
                            if !node.insert_data(tail, value)? {
                                return Ok(false);
                            }
                            node.invalidate_ref_cache();
                        }
                        // if the corresponding child is empty, insert a new leaf
                        None => {
                            *child = Some(Box::new(
                                MptNodeData::Leaf(util::to_encoded_path(tail, true), value).into(),
                            ));
                        }
                    }
                } else {
                    return Err(Error::ValueInBranch);
                }
            }
            MptNodeData::Leaf(prefix, old_value) => {
                let self_nibs = util::prefix_nibs(prefix);
                let common_len = util::lcp(&self_nibs, key_nibs);
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
                    let mut children: [Option<Box<MptNodePointer>>; 16] = Default::default();

                    children[self_nibs[common_len] as usize] = Some(Box::new(
                        MptNodeData::Leaf(
                            util::to_encoded_path(&self_nibs[split_point..], true),
                            mem::take(old_value),
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
                        *self = MptNodeData::Extension(
                            util::to_encoded_path(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        *self = branch;
                    }
                }
            }
            MptNodeData::Extension(prefix, existing_child) => {
                let self_nibs = util::prefix_nibs(prefix);
                let common_len = util::lcp(&self_nibs, key_nibs);
                if common_len == self_nibs.len() {
                    // traverse down for update
                    if !existing_child.insert_data(&key_nibs[common_len..], value)? {
                        return Ok(false);
                    }
                    existing_child.invalidate_ref_cache();
                } else if common_len == key_nibs.len() {
                    return Err(Error::ValueInBranch);
                } else {
                    let split_point = common_len + 1;
                    // otherwise, create a branch with two children
                    let mut children: [Option<Box<MptNodePointer>>; 16] = Default::default();

                    children[self_nibs[common_len] as usize] = if split_point < self_nibs.len() {
                        Some(Box::new(
                            MptNodeData::Extension(
                                util::to_encoded_path(&self_nibs[split_point..], false),
                                mem::take(existing_child),
                            )
                            .into(),
                        ))
                    } else {
                        Some(mem::take(existing_child))
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
                        *self = MptNodeData::Extension(
                            util::to_encoded_path(&self_nibs[..common_len], false),
                            Box::new(branch.into()),
                        );
                    } else {
                        *self = branch;
                    }
                }
            }
            MptNodeData::Digest(digest) => return Err(Error::NodeNotResolved(*digest)),
        };
        Ok(true)
    }

    pub fn delete(&mut self, key_nibs: &[u8]) -> Result<bool, Error> {
        match self {
            MptNodeData::Null => return Ok(false),
            MptNodeData::Branch(children) => {
                if let Some((i, tail)) = key_nibs.split_first() {
                    let child = &mut children[*i as usize];
                    match child {
                        Some(node) => {
                            if !node.delete_data(tail)? {
                                return Ok(false);
                            }
                            if node.is_empty() {
                                // if the node is now empty, remove it
                                *child = None;
                            } else {
                                // invalidate cached node reference
                                node.invalidate_ref_cache();
                            }
                        }
                        None => return Ok(false),
                    }
                } else {
                    return Err(Error::ValueInBranch);
                }

                self.maybe_collapse(None)?;
            }
            MptNodeData::Leaf(prefix, _) => {
                if util::prefix_nibs(prefix) != key_nibs {
                    return Ok(false);
                }
                *self = MptNodeData::Null;
            }
            MptNodeData::Extension(prefix, child) => {
                let self_nibs = util::prefix_nibs(prefix);
                if let Some(tail) = key_nibs.strip_prefix(self_nibs.as_slice()) {
                    if !child.delete_data(tail)? {
                        return Ok(false);
                    }
                    child.invalidate_ref_cache();
                } else {
                    return Ok(false);
                }

                // an extension can only point to a branch or a digest; since it's sub trie was
                // modified, we need to make sure that this property still holds
                self.maybe_collapse(Some(self_nibs))?;
            }
            MptNodeData::Digest(digest) => return Err(Error::NodeNotResolved(*digest)),
        };

        Ok(true)
    }

    pub fn maybe_collapse(&mut self, nibs: Option<Vec<u8>>) -> Result<(), Error> {
        match self {
            MptNodeData::Branch(children) => {
                let mut remaining = children.iter_mut().enumerate().filter(|(_, n)| n.is_some());
                // there will always be at least one remaining node
                let (index, node) = remaining.next().unwrap();
                // if there is only exactly one node left, we need to convert the branch
                if remaining.next().is_none() {
                    let mut orphan = node.take().unwrap();
                    match orphan.as_mut() {
                        MptNodePointer::Ref(orphan_node) => {
                            match &orphan_node.data {
                                // if the orphan is a leaf, prepend the corresponding nib to it
                                ArchivedMptNodeData::Leaf(prefix, orphan_value) => {
                                    let new_nibs: Vec<_> = iter::once(index as u8)
                                        .chain(util::prefix_nibs(prefix.as_slice()))
                                        .collect();
                                    *self = MptNodeData::Leaf(
                                        util::to_encoded_path(&new_nibs, true),
                                        orphan_value.to_vec(),
                                    );
                                }
                                // if the orphan is an extension, prepend the corresponding nib to it
                                ArchivedMptNodeData::Extension(prefix, orphan_child) => {
                                    let new_nibs: Vec<_> = iter::once(index as u8)
                                        .chain(util::prefix_nibs(prefix.as_slice()))
                                        .collect();

                                    *self = MptNodeData::Extension(
                                        util::to_encoded_path(&new_nibs, false),
                                        Box::new(orphan_child.as_ref().into()),
                                    );
                                }
                                // if the orphan is a branch, convert to an extension
                                ArchivedMptNodeData::Branch(_) => {
                                    *self = MptNodeData::Extension(
                                        util::to_encoded_path(&[index as u8], false),
                                        orphan,
                                    );
                                }
                                ArchivedMptNodeData::Digest(digest) => {
                                    return Err(Error::NodeNotResolved(digest.0.into()));
                                }
                                ArchivedMptNodeData::Null => unreachable!(),
                            }
                        }
                        MptNodePointer::Own(orphan_node) => {
                            match &mut orphan_node.data {
                                // if the orphan is a leaf, prepend the corresponding nib to it
                                MptNodeData::Leaf(prefix, orphan_value) => {
                                    let new_nibs: Vec<_> = iter::once(index as u8)
                                        .chain(util::prefix_nibs(prefix))
                                        .collect();
                                    *self = MptNodeData::Leaf(
                                        util::to_encoded_path(&new_nibs, true),
                                        mem::take(orphan_value),
                                    );
                                }
                                // if the orphan is an extension, prepend the corresponding nib to it
                                MptNodeData::Extension(prefix, orphan_child) => {
                                    let new_nibs: Vec<_> = iter::once(index as u8)
                                        .chain(util::prefix_nibs(prefix))
                                        .collect();
                                    *self = MptNodeData::Extension(
                                        util::to_encoded_path(&new_nibs, false),
                                        mem::take(orphan_child),
                                    );
                                }
                                // if the orphan is a branch, convert to an extension
                                MptNodeData::Branch(_) => {
                                    *self = MptNodeData::Extension(
                                        util::to_encoded_path(&[index as u8], false),
                                        orphan,
                                    );
                                }
                                MptNodeData::Digest(digest) => {
                                    return Err(Error::NodeNotResolved(*digest));
                                }
                                MptNodeData::Null => unreachable!(),
                            }
                        }
                    };
                }
            }
            MptNodeData::Extension(_, child) => {
                let mut self_nibs = nibs.unwrap();
                // an extension can only point to a branch or a digest; since it's sub trie was
                // modified, we need to make sure that this property still holds
                match child.as_mut() {
                    MptNodePointer::Ref(child) => {
                        match &child.data {
                            // if the child is empty, remove the extension
                            ArchivedMptNodeData::Null => {
                                *self = MptNodeData::Null;
                            }
                            // for a leaf, replace the extension with the extended leaf
                            ArchivedMptNodeData::Leaf(prefix, value) => {
                                self_nibs.extend(util::prefix_nibs(prefix));
                                *self = MptNodeData::Leaf(
                                    util::to_encoded_path(&self_nibs, true),
                                    value.to_vec(),
                                );
                            }
                            // for an extension, replace the extension with the extended extension
                            ArchivedMptNodeData::Extension(prefix, node) => {
                                self_nibs.extend(util::prefix_nibs(prefix));
                                *self = MptNodeData::Extension(
                                    util::to_encoded_path(&self_nibs, false),
                                    Box::new(node.as_ref().into()),
                                );
                            }
                            // for a branch, the extension is still correct
                            ArchivedMptNodeData::Branch(_) => {}
                            // if the child were a digest an early return should have been hit
                            ArchivedMptNodeData::Digest(_) => unreachable!(),
                        }
                    }
                    MptNodePointer::Own(child) => {
                        match &mut child.data {
                            // if the child is empty, remove the extension
                            MptNodeData::Null => {
                                *self = MptNodeData::Null;
                            }
                            // for a leaf, replace the extension with the extended leaf
                            MptNodeData::Leaf(prefix, value) => {
                                self_nibs.extend(util::prefix_nibs(prefix));
                                *self = MptNodeData::Leaf(
                                    util::to_encoded_path(&self_nibs, true),
                                    mem::take(value),
                                );
                            }
                            // for an extension, replace the extension with the extended extension
                            MptNodeData::Extension(prefix, node) => {
                                self_nibs.extend(util::prefix_nibs(prefix));
                                *self = MptNodeData::Extension(
                                    util::to_encoded_path(&self_nibs, false),
                                    mem::take(node),
                                );
                            }
                            // for a branch, the extension is still correct
                            MptNodeData::Branch(_) => {}
                            // if the child were a digest an early return should have been hit
                            MptNodeData::Digest(_) => unreachable!(),
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}
