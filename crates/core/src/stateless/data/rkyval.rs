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

use alloy_primitives::{Address, Bytes, U256};
use k256::ecdsa::VerifyingKey;
use k256::elliptic_curve::sec1::EncodedPoint;
use k256::Secp256k1;
use reth_chainspec::NamedChain;
use rkyv::vec::{ArchivedVec, VecResolver};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Place};

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
