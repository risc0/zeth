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

pub mod entry;
pub mod rkyval;

use crate::trie::node::MptNode;
use alloy_primitives::map::HashMap;
use alloy_primitives::{Address, Bytes, U256};
use entry::StorageEntry;
use k256::ecdsa::VerifyingKey;
use reth_chainspec::NamedChain;
use serde::{Deserialize, Serialize};

/// External block input.
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct StatelessClientData<Block, Header> {
    /// The chain for this data
    #[rkyv(with = rkyval::EncodeNamedChain)]
    pub chain: NamedChain,
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// List of public keys for transaction signatures
    #[rkyv(with = rkyv::with::Map<rkyv::with::Map<rkyval::EncodeVerifyingKey>>)]
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    #[rkyv(with = rkyv::with::MapKV<rkyval::AddressDef, StorageEntry>)]
    pub storage_tries: HashMap<Address, StorageEntry>,
    /// The code for each account
    #[rkyv(with = rkyv::with::Map<rkyval::EncodeBytes>)]
    pub contracts: Vec<Bytes>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Total difficulty before executing block
    #[rkyv(with = rkyval::U256Def)]
    pub total_difficulty: U256,
}

/// External block input.
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct RkyvStatelessClientData {
    /// The chain for this data
    #[rkyv(with = rkyval::EncodeNamedChain)]
    pub chain: NamedChain,
    /// List of public keys for transaction signatures
    #[rkyv(with = rkyv::with::Map<rkyv::with::Map<rkyval::EncodeVerifyingKey>>)]
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    #[rkyv(with = rkyv::with::MapKV<rkyval::AddressDef, StorageEntry>)]
    pub storage_tries: HashMap<Address, StorageEntry>,
    /// The code for each account
    #[rkyv(with = rkyv::with::Map<rkyval::EncodeBytes>)]
    pub contracts: Vec<Bytes>,
    /// Total difficulty before executing block
    #[rkyv(with = rkyval::U256Def)]
    pub total_difficulty: U256,
}

impl<B, H> From<StatelessClientData<B, H>> for RkyvStatelessClientData {
    fn from(value: StatelessClientData<B, H>) -> Self {
        Self {
            chain: value.chain,
            signers: value.signers,
            state_trie: value.state_trie,
            storage_tries: value.storage_tries,
            contracts: value.contracts,
            total_difficulty: value.total_difficulty,
        }
    }
}

/// External block input.
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct StatelessClientChainData<Block, Header> {
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

impl<B, H> From<StatelessClientData<B, H>> for StatelessClientChainData<B, H> {
    fn from(value: StatelessClientData<B, H>) -> Self {
        Self {
            blocks: value.blocks,
            parent_header: value.parent_header,
            ancestor_headers: value.ancestor_headers,
        }
    }
}

impl<Block, Header> StatelessClientData<Block, Header> {
    pub fn from_parts(
        rkyved: RkyvStatelessClientData,
        chain: StatelessClientChainData<Block, Header>,
    ) -> Self {
        Self {
            chain: rkyved.chain,
            blocks: chain.blocks,
            signers: rkyved.signers,
            state_trie: rkyved.state_trie,
            storage_tries: rkyved.storage_tries,
            contracts: rkyved.contracts,
            parent_header: chain.parent_header,
            ancestor_headers: chain.ancestor_headers,
            total_difficulty: rkyved.total_difficulty,
        }
    }
}
