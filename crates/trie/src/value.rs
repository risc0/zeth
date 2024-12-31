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

pub enum ValuePointer<'a, T: Archive>
where
    Archived<T>: 'a,
{
    Ref(&'a Archived<Vec<T>>),
    Own(Vec<T>),
}

impl<'a, T: Archive> Default for ValuePointer<'a, T> {
    fn default() -> Self {
        Self::Own(vec![])
    }
}

impl<'a, T: Archive + Clone> Clone for ValuePointer<'a, T> {
    fn clone(&self) -> Self {
        match self {
            ValuePointer::Ref(r) => ValuePointer::Ref(r),
            ValuePointer::Own(o) => ValuePointer::Own(o.clone()),
        }
    }
}

impl<T: Archive<Archived = T> + Debug> Debug for ValuePointer<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ValuePointer::Ref(r) => r.fmt(f),
            ValuePointer::Own(o) => o.fmt(f),
        }
    }
}

impl<'a, T: Archive<Archived = T> + PartialEq> PartialEq for ValuePointer<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        match self {
            ValuePointer::Ref(r1) => match other {
                ValuePointer::Ref(r2) => r1.eq(r2),
                ValuePointer::Own(o2) => r1.iter().eq(o2.iter()),
            },
            ValuePointer::Own(o1) => match other {
                ValuePointer::Ref(r2) => o1.iter().eq(r2.iter()),
                ValuePointer::Own(o2) => o1.eq(o2),
            },
        }
    }
}

impl<'a, T: Archive<Archived = T> + PartialEq> Eq for ValuePointer<'a, T> {}

impl<'a, T: Archive> From<&'a Archived<Vec<T>>> for ValuePointer<'a, T> {
    fn from(value: &'a Archived<Vec<T>>) -> Self {
        Self::Ref(value)
    }
}

impl<T: Archive> From<Vec<T>> for ValuePointer<'static, T> {
    fn from(value: Vec<T>) -> Self {
        Self::Own(value)
    }
}

pub struct EncodeVP;

impl<T: Archive> ArchiveWith<ValuePointer<'_, T>> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    type Archived = Archived<Vec<T>>;
    type Resolver = Resolver<Vec<T>>;

    fn resolve_with(
        field: &ValuePointer<'_, T>,
        resolver: Self::Resolver,
        out: Place<Self::Archived>,
    ) {
        match field {
            ValuePointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                o.resolve(resolver, out);
            }
            ValuePointer::Own(o) => o.resolve(resolver, out),
        }
    }
}

impl<S, T: Archive + Serialize<S>> SerializeWith<ValuePointer<'_, T>, S> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
{
    fn serialize_with(
        field: &ValuePointer<'_, T>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        match field {
            ValuePointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                Serialize::serialize(&o, serializer)
            }
            ValuePointer::Own(o) => Serialize::serialize(o, serializer),
        }
    }
}

impl<'a, D, T: Archive> DeserializeWith<Archived<Vec<T>>, ValuePointer<'a, T>, D> for EncodeVP
where
    Archived<Vec<T>>: Deserialize<Vec<T>, D>,
    D: rkyv::rancor::Fallible + ?Sized,
{
    fn deserialize_with(
        field: &Archived<Vec<T>>,
        deserializer: &mut D,
    ) -> Result<ValuePointer<'a, T>, D::Error> {
        Deserialize::<Vec<T>, D>::deserialize(field, deserializer).map(ValuePointer::Own)
    }
}

impl<T: Archive> Archive for ValuePointer<'_, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    type Archived = Archived<Vec<T>>;
    type Resolver = Resolver<Vec<T>>;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        match self {
            ValuePointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                o.resolve(resolver, out);
            }
            ValuePointer::Own(o) => o.resolve(resolver, out),
        }
    }
}

impl<'a, S, T: Archive + Serialize<S>> Serialize<S> for ValuePointer<'a, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
    S: rkyv::rancor::Fallible + rkyv::ser::Allocator + rkyv::ser::Writer + ?Sized,
    <S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        match self {
            ValuePointer::Ref(r) => {
                let o = rkyv::deserialize::<Vec<T>, Error>(*r).unwrap();
                Serialize::serialize(&o, serializer)
            }
            ValuePointer::Own(o) => Serialize::serialize(o, serializer),
        }
    }
}

// impl<'a, D, T: Archive> DeserializeWith<Archived<Vec<T>>, ValuePointer<'a, T>, D> for ValuePointer<'a, T>
// where
//     Archived<Vec<T>>: Deserialize<Vec<T>, D>,
//     D: rkyv::rancor::Fallible + ?Sized,
//     <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
// {
//     fn deserialize_with(field: &Archived<Vec<T>>, deserializer: &mut D) -> Result<ValuePointer<'a, T>, D::Error> {
//         Deserialize::<Vec<T>, D>::deserialize(field, deserializer).map(ValuePointer::Own)
//     }
// }

// impl<'a, D, T: Archive<Archived = T>> Deserialize<ValuePointer<'a, T>, D> for Archived<Vec<T>>
// where
//     Archived<Vec<T>>: Deserialize<Vec<T>, D>,
//     D: rkyv::rancor::Fallible + ?Sized,
//     <D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
// {
//     fn deserialize(&self, deserializer: &mut D) -> Result<ValuePointer<'a, T>, D::Error> {
//         Deserialize::<Vec<T>, D>::deserialize(self, deserializer).map(ValuePointer::Own)
//     }
// }

impl<'de, T: Archive + serde::Deserialize<'de>> serde::Deserialize<'de> for ValuePointer<'_, T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(ValuePointer::Own)
    }
}

impl<T: Archive + serde::Serialize> serde::Serialize for ValuePointer<'_, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ValuePointer::Ref(ptr) => {
                let data = rkyv::deserialize::<Vec<T>, Error>(*ptr).unwrap();
                serde::Serialize::serialize(&data, serializer)
            }
            ValuePointer::Own(data) => serde::Serialize::serialize(&data, serializer),
        }
    }
}

impl<'a, T: Archive + Clone> ValuePointer<'a, T>
where
    Archived<Vec<T>>: Deserialize<Vec<T>, Strategy<Pool, Error>>,
{
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            ValuePointer::Ref(r) => rkyv::deserialize::<Vec<T>, Error>(*r).unwrap(),
            ValuePointer::Own(o) => o.to_vec(),
        }
    }
}

impl<'a, T: Archive<Archived = T>> ValuePointer<'a, T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            ValuePointer::Ref(r) => r.as_slice(),
            ValuePointer::Own(o) => o.as_slice(),
        }
    }
}
