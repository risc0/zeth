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

use crate::mpt::MptNode;
use alloy_primitives::map::HashMap;
use alloy_primitives::{Address, Bytes, U256};
use reth_chainspec::NamedChain;
use serde::{Deserialize, Serialize};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct StatelessClientData<Block, Header> {
    /// The chain for this data
    pub chain: NamedChain,
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub storage_tries: HashMap<Address, StorageEntry>,
    /// The code for each account
    pub contracts: Vec<Bytes>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Total difficulty before executing block
    pub total_difficulty: U256,
}
