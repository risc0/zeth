// Copyright 2023 RISC Zero, Inc.
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

//! Convert to revm types.

use alloy_primitives::{Address, B256};
use revm_primitives::{Log as RevmLog, B160 as RevmB160, B256 as RevmB256, U256 as RevmU256};

use crate::{
    access_list::{AccessList, AccessListItem},
    receipt::Log,
};

/// Converts a `Address` type to its corresponding `RevmB160` representation.
#[inline]
pub fn to_revm_b160(v: Address) -> RevmB160 {
    v.0 .0.into()
}

/// Converts a `B256` type to its corresponding `RevmB256` representation.
#[inline]
pub fn to_revm_b256(v: B256) -> RevmB256 {
    v.0.into()
}

/// Converts a `RevmB160` type to its corresponding `Address` representation.
#[inline]
pub fn from_revm_b160(v: RevmB160) -> Address {
    v.0.into()
}

/// Converts a `RevmB256` type to its corresponding `B256` representation.
#[inline]
pub fn from_revm_b256(v: RevmB256) -> B256 {
    v.0.into()
}

/// Provides a conversion from [AccessListItem] to a tuple of `RevmB160` and a vector of
/// `RevmU256`.
impl From<AccessListItem> for (RevmB160, Vec<RevmU256>) {
    fn from(item: AccessListItem) -> (RevmB160, Vec<RevmU256>) {
        (
            to_revm_b160(item.address),
            item.storage_keys
                .into_iter()
                .map(|item| item.into())
                .collect(),
        )
    }
}

/// Provides a conversion from [AccessList] to a vector of tuples containing `RevmB160`
/// and a vector of `RevmU256`.
impl From<AccessList> for Vec<(RevmB160, Vec<RevmU256>)> {
    fn from(list: AccessList) -> Vec<(RevmB160, Vec<RevmU256>)> {
        list.0.into_iter().map(|item| item.into()).collect()
    }
}

/// Provides a conversion from `RevmLog` to the local [Log].
impl From<RevmLog> for Log {
    fn from(log: RevmLog) -> Self {
        Log {
            address: log.address.to_fixed_bytes().into(),
            topics: log.topics.into_iter().map(from_revm_b256).collect(),
            data: log.data.into(),
        }
    }
}
