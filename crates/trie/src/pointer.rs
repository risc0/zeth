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

use crate::data::MptNodeData;
use crate::node::{ArchivedMptNode, MptNode, MptNodeResolver};
use crate::util::Error;
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable};
use rkyv::Place;
use serde::{Deserializer, Serializer};
use std::fmt::Debug;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MptNodePointer<'a> {
    Ref(&'a ArchivedMptNode<'a>),
    Own(MptNode<'a>),
}

impl<'a> From<&'a ArchivedMptNode<'a>> for MptNodePointer<'a> {
    fn from(value: &'a ArchivedMptNode<'a>) -> Self {
        Self::Ref(value)
    }
}

impl<'a> From<MptNode<'a>> for MptNodePointer<'a> {
    fn from(value: MptNode<'a>) -> Self {
        Self::Own(value)
    }
}

impl<'a> From<MptNodeData<'a>> for MptNodePointer<'a> {
    fn from(value: MptNodeData<'a>) -> Self {
        MptNode::from(value).into()
    }
}

impl<'a> MptNodePointer<'a> {
    #[inline]
    pub fn get(&self, key: &[u8]) -> Result<Option<&[u8]>, Error> {
        match self {
            MptNodePointer::Ref(node) => node.get(key),
            MptNodePointer::Own(node) => node.get(key),
        }
    }

    pub fn data_get(&self, key_nibs: &[u8]) -> Result<Option<&[u8]>, Error> {
        match self {
            MptNodePointer::Ref(node) => node.data.get(key_nibs),
            MptNodePointer::Own(node) => node.data.get(key_nibs),
        }
    }

    #[inline]
    pub fn get_rlp<T: Decodable>(&self, key: &[u8]) -> Result<Option<T>, Error> {
        match self {
            MptNodePointer::Ref(node) => node.get_rlp(key),
            MptNodePointer::Own(node) => node.get_rlp(key),
        }
    }

    #[inline]
    pub fn delete(&mut self, key: &[u8]) -> Result<bool, Error> {
        match self {
            MptNodePointer::Ref(node) => {
                let Some(replacement) = node.delete(key)? else {
                    return Ok(false);
                };
                *self = MptNodePointer::Own(replacement);
                Ok(true)
            }
            MptNodePointer::Own(node) => node.delete(key),
        }
    }

    pub fn data_delete(&mut self, key_nibs: &[u8]) -> Result<bool, Error> {
        match self {
            MptNodePointer::Ref(node) => {
                let Some(replacement) = node.data.delete(key_nibs)? else {
                    return Ok(false);
                };
                *self = MptNodePointer::Own(replacement.into());
                Ok(true)
            }
            MptNodePointer::Own(node) => {
                if !node.data.delete(key_nibs)? {
                    return Ok(false);
                };
                node.invalidate_ref_cache();
                Ok(true)
            }
        }
    }

    #[inline]
    pub fn insert(&mut self, key: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        match self {
            MptNodePointer::Ref(node) => {
                let Some(replacement) = node.insert(key, value)? else {
                    return Ok(false);
                };
                *self = MptNodePointer::Own(replacement);
                Ok(true)
            }
            MptNodePointer::Own(node) => node.insert(key, value),
        }
    }

    pub fn data_insert(&mut self, key_nibs: &[u8], value: Vec<u8>) -> Result<bool, Error> {
        match self {
            MptNodePointer::Ref(node) => {
                let Some(replacement) = node.data.insert(key_nibs, value)? else {
                    return Ok(false);
                };
                *self = MptNodePointer::Own(replacement.into());
                Ok(true)
            }
            MptNodePointer::Own(node) => {
                if !node.data.insert(key_nibs, value)? {
                    return Ok(false);
                };
                node.invalidate_ref_cache();
                Ok(true)
            }
        }
    }

    #[inline]
    pub fn insert_rlp(&mut self, key: &[u8], value: impl Encodable) -> Result<bool, Error> {
        self.insert(key, alloy_rlp::encode(value))
    }

    pub fn clear(&mut self) {
        *self = Default::default();
    }

    #[inline]
    pub fn is_reference_cached(&self) -> bool {
        match self {
            MptNodePointer::Ref(_) => true,
            MptNodePointer::Own(o) => o.is_reference_cached(),
        }
    }

    pub fn invalidate_ref_cache(&mut self) {
        let MptNodePointer::Own(node) = self else {
            unreachable!()
        };
        node.invalidate_ref_cache();
    }

    pub fn is_empty(&self) -> bool {
        match self {
            MptNodePointer::Ref(node) => node.is_empty(),
            MptNodePointer::Own(node) => node.is_empty(),
        }
    }

    pub fn is_digest(&self) -> bool {
        match self {
            MptNodePointer::Ref(node) => node.is_digest(),
            MptNodePointer::Own(node) => node.is_digest(),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            MptNodePointer::Ref(node) => node.size(),
            MptNodePointer::Own(node) => node.size(),
        }
    }

    pub fn debug_rlp<T: alloy_rlp::Decodable + Debug>(&self) -> Vec<String> {
        let MptNodePointer::Own(node) = self else {
            unimplemented!()
        };
        node.debug_rlp::<T>()
    }

    #[inline]
    pub fn hash(&self) -> B256 {
        match self {
            MptNodePointer::Ref(node) => node.hash(),
            MptNodePointer::Own(node) => node.hash(),
        }
    }

    pub fn reference_length(&self) -> usize {
        match self {
            MptNodePointer::Ref(node) => node.reference_length(),
            MptNodePointer::Own(node) => node.reference_length(),
        }
    }

    pub fn reference_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            MptNodePointer::Ref(node) => {
                node.reference_encode(out);
            }
            MptNodePointer::Own(node) => {
                node.reference_encode(out);
            }
        }
    }

    pub fn to_rw(self) -> MptNode<'a> {
        match self {
            MptNodePointer::Ref(field) => {
                rkyv::deserialize::<MptNode, rkyv::rancor::Error>(field).unwrap()
            }
            MptNodePointer::Own(node) => node,
        }
    }

    pub fn as_mut_node(&mut self) -> anyhow::Result<&mut MptNode<'a>> {
        match self {
            MptNodePointer::Own(node) => Ok(node),
            _ => anyhow::bail!("attempted mutable access to read-only ptr"),
        }
    }
}

impl<'a> rkyv::Archive for MptNodePointer<'a> {
    type Archived = ArchivedMptNode<'a>;
    type Resolver = MptNodeResolver<'a>;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        match self {
            MptNodePointer::Ref(field) => {
                let data = rkyv::deserialize::<MptNode, rkyv::rancor::Error>(*field).unwrap();
                data.hash();
                data.resolve(resolver, out);
            }
            MptNodePointer::Own(data) => {
                data.hash();
                data.resolve(resolver, out);
            }
        }
    }
}

impl<'a, S> rkyv::Serialize<S> for MptNodePointer<'a>
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
    <S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        match self {
            MptNodePointer::Ref(field) => {
                let data = rkyv::deserialize::<MptNode, rkyv::rancor::Error>(*field).unwrap();
                data.hash();
                rkyv::Serialize::serialize(&data, serializer)
            }
            MptNodePointer::Own(data) => {
                data.hash();
                rkyv::Serialize::serialize(data, serializer)
            }
        }
    }
}

impl<'a, D> rkyv::Deserialize<MptNodePointer<'a>, D> for ArchivedMptNode<'a>
where
    D: rkyv::rancor::Fallible + ?Sized,
    <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<MptNodePointer<'a>, D::Error> {
        rkyv::Deserialize::<MptNode, D>::deserialize(self, deserializer).map(MptNodePointer::Own)
    }
}

impl<'de> serde::Deserialize<'de> for MptNodePointer<'_> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        MptNode::deserialize(deserializer).map(MptNodePointer::Own)
    }
}

impl serde::Serialize for MptNodePointer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            MptNodePointer::Ref(ptr) => {
                let data = rkyv::deserialize::<MptNode, rkyv::rancor::Error>(*ptr).unwrap();
                data.hash();
                serde::Serialize::serialize(&data, serializer)
            }
            MptNodePointer::Own(data) => {
                data.hash();
                serde::Serialize::serialize(&data, serializer)
            }
        }
    }
}

impl<'a> Default for MptNodePointer<'a> {
    fn default() -> Self {
        Self::Own(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MptNodeData;
    use crate::node::{ArchivedMptNode, MptNode};
    use anyhow::Context;

    #[test]
    pub fn round_trip() -> anyhow::Result<()> {
        let trie = MptNode::from(MptNodeData::Leaf(vec![1, 2, 3], vec![4, 5, 6]));
        let _ = trie.hash();
        let _ = MptNodePointer::Own(trie.clone());
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&trie)?;
        let archived = rkyv::access::<ArchivedMptNode, rkyv::rancor::Error>(&bytes)?;
        archived.verify_reference().context("archived node")?;
        let ptr = MptNodePointer::Ref(&archived);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&ptr)?;
        let archived = rkyv::access::<ArchivedMptNode, rkyv::rancor::Error>(&bytes)?;
        archived.verify_reference().context("archived pointer")?;
        let deserialized = rkyv::deserialize::<MptNodePointer, rkyv::rancor::Error>(archived)?;
        let de_trie = deserialized.to_rw();
        de_trie.hash();
        assert_eq!(trie, de_trie);

        Ok(())
    }
}
