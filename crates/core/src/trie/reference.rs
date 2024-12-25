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

use arrayvec::ArrayVec;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Place};
use std::cell::RefCell;

pub type MptNodeReference = ArrayVec<u8, 32>;

pub type CachedMptRef = RefCell<Option<MptNodeReference>>;

pub struct ForceCachedRef;

impl ArchiveWith<CachedMptRef> for ForceCachedRef {
    type Archived = rkyv::Archived<MptNodeReference>;
    type Resolver = rkyv::Resolver<MptNodeReference>;

    fn resolve_with(field: &CachedMptRef, resolver: Self::Resolver, out: Place<Self::Archived>) {
        field.borrow().as_ref().unwrap().resolve(resolver, out);
    }
}

impl<S> SerializeWith<CachedMptRef, S> for ForceCachedRef
where
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(
        field: &CachedMptRef,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        rkyv::Serialize::serialize(field.borrow().as_ref().unwrap(), serializer)
    }
}

impl<D> DeserializeWith<rkyv::Archived<MptNodeReference>, CachedMptRef, D> for ForceCachedRef
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
