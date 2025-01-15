// Copyright 2024, 2025 RISC Zero, Inc.
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

use crate::mpt::MptNode;
use alloy_primitives::map::AddressHashMap;
use alloy_primitives::{Address, Bytes, U256};
use k256::ecdsa::VerifyingKey;
use k256::elliptic_curve::sec1::EncodedPoint;
use k256::Secp256k1;
use reth_chainspec::NamedChain;
use rkyv::vec::{ArchivedVec, VecResolver};
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
    #[rkyv(with = rkyv::with::Map<U256Def>)]
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

/// External block input.
#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct _StatelessClientData<Block, Header> {
    /// The chain for this data
    pub chain: NamedChain,
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// List of public keys for transaction signatures
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub storage_tries: AddressHashMap<StorageEntry>,
    /// The code for each account
    pub contracts: Vec<Bytes>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Total difficulty before executing block
    pub total_difficulty: U256,
}

/// External block input.
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
pub struct StatelessClientData<Block, Header> {
    /// The chain for this data
    #[rkyv(with = EncodeNamedChain)]
    pub chain: NamedChain,
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// List of public keys for transaction signatures
    #[rkyv(with = rkyv::with::Map<rkyv::with::Map<EncodeVerifyingKey>>)]
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    #[rkyv(with = rkyv::with::MapKV<AddressDef, StorageEntry>)]
    pub storage_tries: AddressHashMap<StorageEntry>,
    /// The code for each account
    #[rkyv(with = rkyv::with::Map<EncodeBytes>)]
    pub contracts: Vec<Bytes>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Total difficulty before executing block
    #[rkyv(with = U256Def)]
    pub total_difficulty: U256,
}

/// External block input.
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
pub struct RkyvStatelessClientData {
    /// The chain for this data
    #[rkyv(with = EncodeNamedChain)]
    pub chain: NamedChain,
    /// List of public keys for transaction signatures
    #[rkyv(with = rkyv::with::Map<rkyv::with::Map<EncodeVerifyingKey>>)]
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    #[rkyv(with = rkyv::with::MapKV<AddressDef, StorageEntry>)]
    pub storage_tries: AddressHashMap<StorageEntry>,
    /// The code for each account
    #[rkyv(with = rkyv::with::Map<EncodeBytes>)]
    pub contracts: Vec<Bytes>,
    /// Total difficulty before executing block
    #[rkyv(with = U256Def)]
    pub total_difficulty: U256,
}

impl<B, H> From<StatelessClientData<B, H>> for RkyvStatelessClientData {
    fn from(value: StatelessClientData<B, H>) -> Self {
        Self {
            chain: value.chain,
            signers: value.signers,
            state_trie: value.state_trie,
            storage_tries: value.storage_tries,
            contracts: value.contracts,
            total_difficulty: value.total_difficulty,
        }
    }
}

/// External block input.
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
pub struct StatelessClientChainData<Block, Header> {
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

impl<B, H> From<StatelessClientData<B, H>> for StatelessClientChainData<B, H> {
    fn from(value: StatelessClientData<B, H>) -> Self {
        Self {
            blocks: value.blocks,
            parent_header: value.parent_header,
            ancestor_headers: value.ancestor_headers,
        }
    }
}

impl<Block, Header> StatelessClientData<Block, Header> {
    pub fn from_parts(
        rkyved: RkyvStatelessClientData,
        chain: StatelessClientChainData<Block, Header>,
    ) -> Self {
        Self {
            chain: rkyved.chain,
            blocks: chain.blocks,
            signers: rkyved.signers,
            state_trie: rkyved.state_trie,
            storage_tries: rkyved.storage_tries,
            contracts: rkyved.contracts,
            parent_header: chain.parent_header,
            ancestor_headers: chain.ancestor_headers,
            total_difficulty: rkyved.total_difficulty,
        }
    }
}

pub struct EncodeNamedChain;

impl ArchiveWith<NamedChain> for EncodeNamedChain {
    type Archived = rkyv::Archived<u64>;
    type Resolver = rkyv::Resolver<u64>;

    fn resolve_with(field: &NamedChain, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let val: u64 = (*field).into();
        val.resolve(resolver, out);
    }
}

impl<S> SerializeWith<NamedChain, S> for EncodeNamedChain
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(field: &NamedChain, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        let val: u64 = (*field).into();
        rkyv::Serialize::serialize(&val, serializer)
    }
}

impl<D> DeserializeWith<rkyv::Archived<u64>, NamedChain, D> for EncodeNamedChain
where
    D: rkyv::rancor::Fallible + ?Sized,
{
    fn deserialize_with(field: &rkyv::Archived<u64>, _: &mut D) -> Result<NamedChain, D::Error> {
        Ok(NamedChain::try_from(field.to_native()).unwrap())
    }
}

pub struct EncodeVerifyingKey;

impl ArchiveWith<VerifyingKey> for EncodeVerifyingKey {
    type Archived = ArchivedVec<u8>;
    type Resolver = VecResolver;

    fn resolve_with(field: &VerifyingKey, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let encoded = field.to_encoded_point(false).to_bytes().to_vec();
        encoded.resolve(resolver, out);
    }
}

impl<S> SerializeWith<VerifyingKey, S> for EncodeVerifyingKey
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(
        field: &VerifyingKey,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        let encoded = field.to_encoded_point(false).to_bytes().to_vec();
        rkyv::Serialize::serialize(&encoded, serializer)
    }
}

impl<D> DeserializeWith<ArchivedVec<u8>, VerifyingKey, D> for EncodeVerifyingKey
where
    D: rkyv::rancor::Fallible + ?Sized,
{
    fn deserialize_with(field: &ArchivedVec<u8>, _: &mut D) -> Result<VerifyingKey, D::Error> {
        let encoded_point = EncodedPoint::<Secp256k1>::from_bytes(field.as_slice()).unwrap();
        Ok(VerifyingKey::from_encoded_point(&encoded_point).unwrap())
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = Address)]
#[rkyv(archived = ArchivedAddress)]
#[rkyv(derive(Hash, Eq, PartialEq))]
pub struct AddressDef(pub [u8; 20]);

impl From<AddressDef> for Address {
    fn from(value: AddressDef) -> Self {
        Address::new(value.0)
    }
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = U256)]
#[rkyv(archived = ArchivedU256)]
pub struct U256Def {
    #[rkyv(getter = U256::as_limbs)]
    limbs: [u64; 4],
}

impl From<U256Def> for U256 {
    fn from(value: U256Def) -> Self {
        U256::from_limbs(value.limbs)
    }
}

pub struct EncodeBytes;

impl ArchiveWith<Bytes> for EncodeBytes {
    type Archived = ArchivedVec<u8>;
    type Resolver = VecResolver;

    fn resolve_with(field: &Bytes, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedVec::<u8>::resolve_from_slice(field.0.as_ref(), resolver, out);
    }
}

impl<S> SerializeWith<Bytes, S> for EncodeBytes
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(field: &Bytes, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        rkyv::Serialize::serialize(&field.0.to_vec(), serializer)
    }
}

impl<D> DeserializeWith<rkyv::Archived<Vec<u8>>, Bytes, D> for EncodeBytes
where
    D: rkyv::rancor::Fallible + ?Sized,
{
    fn deserialize_with(field: &rkyv::Archived<Vec<u8>>, _: &mut D) -> Result<Bytes, D::Error> {
        let res = Bytes::copy_from_slice(field.as_slice());
        Ok(res)
    }
}
