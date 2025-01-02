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

use crate::stateless::data::entry::StorageEntryPointer;
use crate::stateless::data::rkyval::{EncodeNamedChain, EncodeVerifyingKey, U256Def};
use alloy_primitives::map::HashMap;
use alloy_primitives::{Address, Bytes, U256};
use entry::StorageEntry;
use k256::ecdsa::VerifyingKey;
use reth_chainspec::NamedChain;
use rkyv::de::Pool;
use rkyv::rancor::{Failure, Strategy};
use rkyv::with::DeserializeWith;
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, Hasher};
use zeth_trie::node::MptNode;
use zeth_trie::pointer::MptNodePointer;
use zeth_trie::value::ValuePointer;

/// External block input.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StatelessClientData<'a, Block, Header> {
    /// The chain for this data
    pub chain: NamedChain,
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// List of public keys for transaction signatures
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNodePointer<'a>,
    /// Maps each address with its storage trie and the used storage slots.
    pub storage_tries: HashMap<Address, StorageEntryPointer<'a>, NoHasherBuilder>,
    /// The code for each account
    pub contracts: Vec<ValuePointer<'a, u8>>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Total difficulty before executing block
    pub total_difficulty: U256,
}

impl<'a, Block, Header> StatelessClientData<'a, Block, Header> {
    pub fn from_parts(common: CommonData<'a>, chain: ChainData<Block, Header>) -> Self {
        Self {
            chain: common.chain,
            blocks: chain.blocks,
            signers: common.signers,
            state_trie: common.state_trie.into(),
            storage_tries: common
                .storage_tries
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            contracts: common
                .contracts
                .into_iter()
                .map(|c| c.to_vec().into())
                .collect(),
            parent_header: chain.parent_header,
            ancestor_headers: chain.ancestor_headers,
            total_difficulty: common.total_difficulty,
        }
    }

    pub fn from_rkyv(
        common: &'a ArchivedCommonData<'a>,
        chain: ChainData<Block, Header>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            chain: EncodeNamedChain::deserialize_with(
                &common.chain,
                Strategy::<_, Failure>::wrap(&mut Pool::new()),
            )?,
            blocks: chain.blocks,
            signers: common
                .signers
                .iter()
                .map(|v| {
                    v.iter()
                        .map(|v| {
                            EncodeVerifyingKey::deserialize_with(
                                v,
                                Strategy::<_, Failure>::wrap(&mut Pool::new()),
                            )
                            .unwrap()
                        })
                        .collect()
                })
                .collect(),
            state_trie: {
                common
                    .state_trie
                    .verify_reference()
                    .expect("Failed to validate state trie.");
                (&common.state_trie).into()
            },
            storage_tries: common
                .storage_tries
                .iter()
                .map(|(k, v)| {
                    v.storage_trie
                        .verify_reference()
                        .expect("Failed to validate storage trie.");
                    (
                        Address::from(k.0),
                        StorageEntryPointer {
                            storage_trie: MptNodePointer::Ref(&v.storage_trie),
                            slots: rkyv::with::Map::<U256Def>::deserialize_with(
                                &v.slots,
                                Strategy::<_, Failure>::wrap(&mut Pool::new())
                            ).unwrap(),
                        },
                    )
                })
                .collect(),
            contracts: common.contracts.iter().map(|c| c.into()).collect(),
            parent_header: chain.parent_header,
            ancestor_headers: chain.ancestor_headers,
            total_difficulty: U256Def::deserialize_with(
                &common.total_difficulty,
                Strategy::<_, Failure>::wrap(&mut Pool::new()),
            )?,
        })
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
pub struct CommonData<'a> {
    /// The chain for this data
    #[rkyv(with = rkyval::EncodeNamedChain)]
    pub chain: NamedChain,
    /// List of public keys for transaction signatures
    #[rkyv(with = rkyv::with::Map<rkyv::with::Map<rkyval::EncodeVerifyingKey>>)]
    pub signers: Vec<Vec<VerifyingKey>>,
    /// State trie of the parent block.
    pub state_trie: MptNode<'a>,
    /// Maps each address with its storage trie and the used storage slots.
    #[rkyv(with = rkyv::with::MapKV<rkyval::AddressDef, StorageEntry<'a>>)]
    pub storage_tries: HashMap<Address, StorageEntry<'a>, NoHasherBuilder>,
    /// The code for each account
    #[rkyv(with = rkyv::with::Map<rkyval::EncodeBytes>)]
    pub contracts: Vec<Bytes>,
    /// Total difficulty before executing block
    #[rkyv(with = rkyval::U256Def)]
    pub total_difficulty: U256,
}

#[derive(Clone, Default)]
pub struct NoHasherBuilder;

impl BuildHasher for NoHasherBuilder {
    type Hasher = NoHasher;

    fn build_hasher(&self) -> Self::Hasher {
        NoHasher::default()
    }
}

#[derive(Default)]
pub struct NoHasher([u8; 8]);

impl Hasher for NoHasher {
    fn finish(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    fn write(&mut self, bytes: &[u8]) {
        let l = std::cmp::min(8, bytes.len());
        self.0[..l].copy_from_slice(&bytes[..l]);
    }
}

impl<'a, B, H> From<StatelessClientData<'a, B, H>> for CommonData<'a> {
    fn from(value: StatelessClientData<'a, B, H>) -> Self {
        Self {
            chain: value.chain,
            signers: value.signers,
            state_trie: value.state_trie.to_rw(),
            storage_tries: value
                .storage_tries
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            contracts: value
                .contracts
                .into_iter()
                .map(|c| c.to_vec().into())
                .collect(),
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
pub struct ChainData<Block, Header> {
    /// Block and transaction data to execute
    pub blocks: Vec<Block>,
    /// Immediate parent header
    pub parent_header: Header,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

impl<B, H> From<StatelessClientData<'_, B, H>> for ChainData<B, H> {
    fn from(value: StatelessClientData<B, H>) -> Self {
        Self {
            blocks: value.blocks,
            parent_header: value.parent_header,
            ancestor_headers: value.ancestor_headers,
        }
    }
}
