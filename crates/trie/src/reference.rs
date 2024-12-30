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

use crate::keccak::keccak;
use alloy_consensus::EMPTY_ROOT_HASH;
use alloy_primitives::{keccak256, B256};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Place};
use std::cell::RefCell;

/// Represents the ways in which one node can reference another node inside the sparse
/// Merkle Patricia Trie (MPT).
///
/// Nodes in the MPT can reference other nodes either directly through their byte
/// representation or indirectly through a hash of their encoding. This enum provides a
/// clear and type-safe way to represent these references.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Ord,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[rkyv(derive(Debug, Eq, PartialEq))]
pub enum MptNodeReference {
    /// Represents a direct reference to another node using its byte encoding.
    /// Used for short encodings that are less than 32 bytes in length.
    Bytes(Vec<u8>),
    /// Represents an indirect reference to another node using the Keccak hash of its encoding.
    /// Used for encodings that are not less than 32 bytes in length.
    Digest(#[rkyv(with = crate::util::B256Def)] B256),
}

impl From<B256> for MptNodeReference {
    fn from(value: B256) -> Self {
        Self::Digest(value)
    }
}

impl From<Vec<u8>> for MptNodeReference {
    fn from(value: Vec<u8>) -> Self {
        if value.len() < 32 {
            Self::Bytes(value)
        } else {
            MptNodeReference::from(B256::from(keccak(&value)))
        }
    }
}

impl MptNodeReference {
    pub fn is_digest(&self) -> bool {
        matches!(self, Self::Digest(_))
    }

    pub fn to_digest(&self) -> B256 {
        match self {
            MptNodeReference::Bytes(b) => keccak256(b),
            MptNodeReference::Digest(d) => *d,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            MptNodeReference::Bytes(b) => b.as_slice(),
            MptNodeReference::Digest(d) => d.as_slice(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            MptNodeReference::Bytes(b) => b.len(),
            MptNodeReference::Digest(_) => 33, // length prefix + 32 bytes of data
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            MptNodeReference::Bytes(b) => b.is_empty(),
            MptNodeReference::Digest(_) => false,
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

pub type CachedMptRef = RefCell<Option<MptNodeReference>>;

pub struct RequireCachedRef;

impl ArchiveWith<CachedMptRef> for RequireCachedRef {
    type Archived = rkyv::Archived<MptNodeReference>;
    type Resolver = rkyv::Resolver<MptNodeReference>;

    fn resolve_with(field: &CachedMptRef, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let digest = field
            .borrow()
            .clone()
            .unwrap_or(MptNodeReference::from(EMPTY_ROOT_HASH));
        digest.resolve(resolver, out);
    }
}

impl<S> SerializeWith<CachedMptRef, S> for RequireCachedRef
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(
        field: &CachedMptRef,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        let digest = field
            .borrow()
            .clone()
            .unwrap_or(MptNodeReference::from(EMPTY_ROOT_HASH));
        rkyv::Serialize::serialize(&digest, serializer)
    }
}

impl<D> DeserializeWith<rkyv::Archived<MptNodeReference>, CachedMptRef, D> for RequireCachedRef
where
    D: rkyv::rancor::Fallible + ?Sized,
    <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn deserialize_with(
        _field: &rkyv::Archived<MptNodeReference>,
        _deserializer: &mut D,
    ) -> Result<CachedMptRef, D::Error> {
        // let res = rkyv::Deserialize::deserialize(field, deserializer)?;
        // Ok(RefCell::new(Some(res)))
        Ok(RefCell::new(None))
    }
}
