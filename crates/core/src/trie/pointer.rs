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

use crate::trie::data::{ArchivedMptNodeData, MptNodeData, MptNodeDataResolver};
use rkyv::Place;
use serde::{Deserializer, Serializer};

#[derive(Clone)]
pub enum MptNodePointer<'a> {
    ReadOnly(&'a ArchivedMptNodeData),
    ReadWrite(MptNodeData),
}

impl MptNodePointer<'_> {
    pub fn data(self) -> MptNodeData {
        match self {
            MptNodePointer::ReadOnly(field) => {
                rkyv::deserialize::<MptNodeData, rkyv::rancor::Error>(field).unwrap()
            }
            MptNodePointer::ReadWrite(data) => data,
        }
    }
}

impl<'a> rkyv::Archive for MptNodePointer<'a> {
    type Archived = ArchivedMptNodeData;
    type Resolver = MptNodeDataResolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        match self {
            MptNodePointer::ReadOnly(field) => {
                let data = rkyv::deserialize::<MptNodeData, rkyv::rancor::Error>(*field).unwrap();
                data.resolve(resolver, out);
            }
            MptNodePointer::ReadWrite(data) => {
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
            MptNodePointer::ReadOnly(field) => {
                let data = rkyv::deserialize::<MptNodeData, rkyv::rancor::Error>(*field).unwrap();
                rkyv::Serialize::serialize(&data, serializer)
            }
            MptNodePointer::ReadWrite(data) => rkyv::Serialize::serialize(data, serializer),
        }
    }
}

impl<'a, D> rkyv::Deserialize<MptNodePointer<'a>, D> for ArchivedMptNodeData
where
    D: rkyv::rancor::Fallible + ?Sized,
    <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<MptNodePointer<'a>, D::Error> {
        rkyv::Deserialize::<MptNodeData, D>::deserialize(self, deserializer)
            .map(MptNodePointer::ReadWrite)
    }
}

impl<'de> serde::Deserialize<'de> for MptNodePointer<'_> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        MptNodeData::deserialize(deserializer).map(MptNodePointer::ReadWrite)
    }
}

impl serde::Serialize for MptNodePointer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            MptNodePointer::ReadOnly(ptr) => {
                let data = rkyv::deserialize::<MptNodeData, rkyv::rancor::Error>(*ptr).unwrap();
                serde::Serialize::serialize(&data, serializer)
            }
            MptNodePointer::ReadWrite(data) => serde::Serialize::serialize(&data, serializer),
        }
    }
}

impl<'a> Default for MptNodePointer<'a> {
    fn default() -> Self {
        Self::ReadWrite(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trie::node::{ArchivedMptNode, MptNode};

    #[test]
    pub fn round_trip() -> anyhow::Result<()> {
        let trie = MptNode::from(MptNodeData::Leaf(vec![1, 2, 3], vec![4, 5, 6]));
        let _ = MptNodePointer::ReadWrite(trie.data.clone());
        let _ = trie.hash();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&trie)?;
        let archived = rkyv::access::<ArchivedMptNode, rkyv::rancor::Error>(&bytes)?;
        let ptr = MptNodePointer::ReadOnly(&archived.data);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&ptr)?;
        let archived = rkyv::access::<ArchivedMptNodeData, rkyv::rancor::Error>(&bytes)?;
        let deserialized = rkyv::deserialize::<MptNodePointer, rkyv::rancor::Error>(archived)?;
        assert_eq!(trie.data, deserialized.data());

        Ok(())
    }
}
