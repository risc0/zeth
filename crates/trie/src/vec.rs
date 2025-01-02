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

use rkyv::de::Pool;
use rkyv::rancor::{Error, Strategy};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Archived, Deserialize, Place, Resolver, Serialize};
use std::fmt::{Debug, Formatter};

pub enum VecPointer<'a, T: Archive>
where
    Archived<T>: 'a,
{
    Ref(&'a Archived<Vec<T>>),
    Own(Vec<T>),
}

impl<'a, T: Archive> Default for VecPointer<'a, T> {
    fn default() -> Self {
        Self::Own(vec![])
    }
}

impl<'a, T: Archive + Clone> Clone for VecPointer<'a, T> {
    fn clone(&self) -> Self {
        match self {
            VecPointer::Ref(r) => VecPointer::Ref(r),
            VecPointer::Own(o) => VecPointer::Own(o.clone()),
        }
    }
}

impl<T: Archive + Debug> Debug for VecPointer<'_, T>
where
    Archived<Vec<T>>: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VecPointer::Ref(r) => r.fmt(f),
            VecPointer::Own(o) => o.fmt(f),
        }
    }
}

impl<'a, T: Archive + PartialEq + PartialEq<Archived<T>>> PartialEq for VecPointer<'a, T>
where
    Archived<T>: PartialEq + PartialEq<T>,
{
    fn eq(&self, other: &Self) -> bool {
        match self {
            VecPointer::Ref(r1) => match other {
                VecPointer::Ref(r2) => r1.eq(r2),
                VecPointer::Own(o2) => r1.iter().eq(o2.iter()),
            },
            VecPointer::Own(o1) => match other {
                VecPointer::Ref(r2) => o1.iter().eq(r2.iter()),
                VecPointer::Own(o2) => o1.eq(o2),
            },
        }
    }
}

impl<'a, T: Archive + PartialEq + PartialEq<Archived<T>>> Eq for VecPointer<'a, T> where
    Archived<T>: PartialEq + PartialEq<T>
{
}

impl<'a, T: Archive> From<&'a Archived<Vec<T>>> for VecPointer<'a, T> {
    fn from(value: &'a Archived<Vec<T>>) -> Self {
        Self::Ref(value)
    }
}

impl<T: Archive> From<Vec<T>> for VecPointer<'static, T> {
    fn from(value: Vec<T>) -> Self {
        Self::Own(value)
    }
}

pub struct EncodeVP;

impl<T: Archive> ArchiveWith<VecPointer<'_, T>> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    type Archived = Archived<Vec<T>>;
    type Resolver = Resolver<Vec<T>>;

    fn resolve_with(
        field: &VecPointer<'_, T>,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        match field {
            VecPointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                o.resolve(resolver, out);
            }
            VecPointer::Own(o) => o.resolve(resolver, out),
        }
    }
}

impl<S, T: Archive + Serialize<S>> SerializeWith<VecPointer<'_, T>, S> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(
        field: &VecPointer<'_, T>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        match field {
            VecPointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                Serialize::serialize(&o, serializer)
            }
            VecPointer::Own(o) => Serialize::serialize(o, serializer),
        }
    }
}

impl<'a, D, T: Archive> DeserializeWith<Archived<Vec<T>>, VecPointer<'a, T>, D> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, D>,
    D: rkyv::rancor::Fallible + ?Sized,
{
    fn deserialize_with(
        field: &Archived<Vec<T>>,
        deserializer: &mut D,
    ) -> Result<VecPointer<'a, T>, D::Error> {
        Deserialize::<Vec<T>, D>::deserialize(field, deserializer).map(VecPointer::Own)
    }
}

impl<T: Archive> Archive for VecPointer<'_, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    type Archived = Archived<Vec<T>>;
    type Resolver = Resolver<Vec<T>>;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        match self {
            VecPointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                o.resolve(resolver, out);
            }
            VecPointer::Own(o) => o.resolve(resolver, out),
        }
    }
}

impl<'a, S, T: Archive + Serialize<S>> Serialize<S> for VecPointer<'a, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
    <S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        match self {
            VecPointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                Serialize::serialize(&o, serializer)
            }
            VecPointer::Own(o) => Serialize::serialize(o, serializer),
        }
    }
}

impl<'de, T: Archive + serde::Deserialize<'de>> serde::Deserialize<'de> for VecPointer<'_, T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(VecPointer::Own)
    }
}

impl<T: Archive + serde::Serialize> serde::Serialize for VecPointer<'_, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            VecPointer::Ref(ptr) => {
                let data = rkyv::deserialize::<Vec<T>, Error>(*ptr).unwrap();
                serde::Serialize::serialize(&data, serializer)
            }
            VecPointer::Own(data) => serde::Serialize::serialize(&data, serializer),
        }
    }
}

impl<'a, T: Archive + Clone> VecPointer<'a, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            VecPointer::Ref(r) => rkyv::deserialize::<Vec<T>, Error>(*r).unwrap(),
            VecPointer::Own(o) => o.to_vec(),
        }
    }
}

impl<'a, T: Archive<Archived = T>> VecPointer<'a, T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            VecPointer::Ref(r) => r.as_slice(),
            VecPointer::Own(o) => o.as_slice(),
        }
    }
}
