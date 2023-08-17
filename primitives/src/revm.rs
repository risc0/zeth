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

use alloy_primitives::{B160, B256};
use revm_primitives::{
    AccountInfo, Log as RevmLog, B160 as RevmB160, B256 as RevmB256, U256 as RevmU256,
};

use crate::{
    access_list::{AccessList, AccessListItem},
    receipt::Log,
    trie::StateAccount,
};

#[inline]
pub fn to_revm_b160(v: B160) -> RevmB160 {
    v.0.into()
}

#[inline]
pub fn to_revm_b256(v: B256) -> RevmB256 {
    v.0.into()
}

#[inline]
pub fn from_revm_b160(v: RevmB160) -> B160 {
    v.0.into()
}

#[inline]
pub fn from_revm_b256(v: RevmB256) -> B256 {
    v.0.into()
}

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

impl From<AccessList> for Vec<(RevmB160, Vec<RevmU256>)> {
    fn from(list: AccessList) -> Vec<(RevmB160, Vec<RevmU256>)> {
        list.0.into_iter().map(|item| item.into()).collect()
    }
}

impl From<RevmLog> for Log {
    fn from(log: RevmLog) -> Self {
        Log {
            address: log.address.to_fixed_bytes().into(),
            topics: log.topics.into_iter().map(from_revm_b256).collect(),
            data: log.data.into(),
        }
    }
}

impl From<StateAccount> for AccountInfo {
    fn from(value: StateAccount) -> Self {
        AccountInfo {
            balance: value.balance,
            nonce: value.nonce,
            code_hash: to_revm_b256(value.code_hash),
            code: None,
        }
    }
}
