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

use crate::trie::node::MptNode;
use alloy_primitives::U256;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Place};
use serde::{Deserialize, Serialize};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct StorageEntry {
    pub storage_trie: MptNode,
    #[rkyv(with = rkyv::with::Map<crate::stateless::data::rkyval::U256Def>)]
    pub slots: Vec<U256>,
}

impl ArchiveWith<StorageEntry> for StorageEntry {
    type Archived = ArchivedStorageEntry;
    type Resolver = StorageEntryResolver;

    fn resolve_with(field: &StorageEntry, resolver: Self::Resolver, out: Place<Self::Archived>) {
        field.resolve(resolver, out);
    }
}

impl<S> SerializeWith<StorageEntry, S> for StorageEntry
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
    <S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn serialize_with(
        field: &StorageEntry,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        rkyv::Serialize::serialize(field, serializer)
    }
}

impl<D> DeserializeWith<ArchivedStorageEntry, StorageEntry, D> for StorageEntry
where
    D: rkyv::rancor::Fallible + ?Sized,
    <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn deserialize_with(
        field: &ArchivedStorageEntry,
        deserializer: &mut D,
    ) -> Result<StorageEntry, D::Error> {
        rkyv::Deserialize::deserialize(field, deserializer)
    }
}
