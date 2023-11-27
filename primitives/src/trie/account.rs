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

use alloy_primitives::{TxNumber, B256, U256};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable, RlpMaxEncodedLen};
use serde::{Deserialize, Serialize};

use crate::{keccak::KECCAK_EMPTY, trie::EMPTY_ROOT};

/// Represents an Ethereum account within the state trie.
///
/// The `StateAccount` struct encapsulates key details of an Ethereum account, including
/// its nonce, balance, storage root, and the hash of its associated bytecode. This
/// representation is used when interacting with or querying the Ethereum state trie.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    RlpEncodable,
    RlpDecodable,
    RlpMaxEncodedLen,
)]
pub struct StateAccount {
    /// The number of transactions sent from this account's address.
    pub nonce: TxNumber,
    /// The current balance of the account in Wei.
    pub balance: U256,
    /// The root of the account's storage trie, representing all stored contract data.
    pub storage_root: B256,
    /// The Keccak-256 hash of the account's associated bytecode (if it's a contract).
    pub code_hash: B256,
}

impl Default for StateAccount {
    /// Provides default values for a [StateAccount].
    ///
    /// The default account has a nonce of 0, a balance of 0 Wei, an empty storage root,
    /// and an empty bytecode hash.
    fn default() -> Self {
        Self {
            nonce: 0,
            balance: U256::ZERO,
            storage_root: EMPTY_ROOT,
            code_hash: KECCAK_EMPTY,
        }
    }
}
