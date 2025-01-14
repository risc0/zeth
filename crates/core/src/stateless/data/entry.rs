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

use alloy_primitives::U256;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Place};
use zeth_trie::node::MptNode;
use zeth_trie::pointer::MptNodePointer;

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    serde::Deserialize,
    serde::Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct StorageEntry<'a> {
    pub storage_trie: MptNode<'a>,
    #[rkyv(with = rkyv::with::Map<crate::stateless::data::rkyval::U256Def>)]
    pub slots: Vec<U256>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StorageEntryPointer<'a> {
    pub storage_trie: MptNodePointer<'a>,
    pub slots: Vec<U256>,
}

impl<'a> From<StorageEntry<'a>> for StorageEntryPointer<'a> {
    fn from(value: StorageEntry<'a>) -> Self {
        Self {
            storage_trie: value.storage_trie.into(),
            slots: value.slots,
        }
    }
}

impl<'a> From<StorageEntryPointer<'a>> for StorageEntry<'a> {
    fn from(value: StorageEntryPointer<'a>) -> Self {
        Self {
            storage_trie: value.storage_trie.to_rw(),
            slots: value.slots,
        }
    }
}

impl<'a> ArchiveWith<StorageEntry<'a>> for StorageEntry<'a> {
    type Archived = ArchivedStorageEntry<'a>;
    type Resolver = StorageEntryResolver<'a>;

    fn resolve_with(
        field: &StorageEntry<'a>,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        field.resolve(resolver, out);
    }
}

impl<'a, S> SerializeWith<StorageEntry<'a>, S> for StorageEntry<'a>
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
    <S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn serialize_with(
        field: &StorageEntry<'a>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        rkyv::Serialize::serialize(field, serializer)
    }
}

impl<'a, D> DeserializeWith<ArchivedStorageEntry<'a>, StorageEntry<'a>, D> for StorageEntry<'a>
where
    D: rkyv::rancor::Fallible + ?Sized,
    <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn deserialize_with(
        field: &ArchivedStorageEntry<'a>,
        deserializer: &mut D,
    ) -> Result<StorageEntry<'a>, D::Error> {
        rkyv::Deserialize::deserialize(field, deserializer)
    }
}
