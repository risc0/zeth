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

/// An Ethereum account as represented in the trie.
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable, RlpMaxEncodedLen)]
pub struct StateAccount {
    /// Account nonce.
    pub nonce: TxNumber,
    /// Account balance.
    pub balance: U256,
    /// Account's storage root.
    pub storage_root: B256,
    /// Hash of the account's bytecode.
    pub code_hash: B256,
}

impl Default for StateAccount {
    fn default() -> Self {
        Self {
            nonce: 0,
            balance: U256::ZERO,
            storage_root: EMPTY_ROOT,
            code_hash: KECCAK_EMPTY,
        }
    }
}
