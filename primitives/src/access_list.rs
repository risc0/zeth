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

use alloy_primitives::{Address, StorageKey};
use alloy_rlp_derive::{RlpDecodable, RlpDecodableWrapper, RlpEncodable, RlpEncodableWrapper};
use serde::{Deserialize, Serialize};

/// Represents an access list as defined in EIP-2930.
///
/// An access list is a list of addresses and storage keys that a transaction will access,
/// allowing for gas optimizations. This structure is introduced to improve the gas cost
/// calculations by making certain accesses cheaper if they are declared in this list.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    RlpEncodableWrapper,
    RlpDecodableWrapper,
)]
pub struct AccessList(pub Vec<AccessListItem>);

/// Represents an item in the [AccessList].
///
/// Each item specifies an Ethereum address and a set of storage keys that the transaction
/// will access. By providing this information up front, the transaction can benefit from
/// gas cost optimizations.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, RlpEncodable, Serialize, Deserialize, RlpDecodable,
)]
pub struct AccessListItem {
    /// The Ethereum address that the transaction will access.
    pub address: Address,
    /// A list of storage keys associated with the given address that the transaction will
    /// access.
    pub storage_keys: Vec<StorageKey>,
}
