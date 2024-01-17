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

//! Convert to revm types.

use alloy_primitives::{Address, U256};
use revm_primitives::Log as RevmLog;

use crate::{
    access_list::{AccessList, AccessListItem},
    receipt::Log,
};

/// Provides a conversion from [AccessListItem] to a tuple of `Address` and a vector of
/// `U256`.
impl From<AccessListItem> for (Address, Vec<U256>) {
    fn from(item: AccessListItem) -> (Address, Vec<U256>) {
        (
            item.address,
            item.storage_keys
                .into_iter()
                .map(|item| item.into())
                .collect(),
        )
    }
}

/// Provides a conversion from [AccessList] to a vector of tuples containing `Address` and
/// a vector of `U256`.
impl From<AccessList> for Vec<(Address, Vec<U256>)> {
    fn from(list: AccessList) -> Vec<(Address, Vec<U256>)> {
        list.0.into_iter().map(|item| item.into()).collect()
    }
}

/// Provides a conversion from `RevmLog` to the local [Log].
impl From<RevmLog> for Log {
    fn from(log: RevmLog) -> Self {
        Log {
            address: log.address,
            topics: log.data.topics().to_vec(),
            data: log.data.data,
        }
    }
}
