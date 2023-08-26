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

use anyhow::Result;
use hashbrown::HashMap;
use revm::primitives::Address;
use zeth_primitives::{
    block::Header,
    keccak::keccak,
    revm::from_revm_b256,
    trie::{MptNode, StateAccount},
    U256,
};

use crate::{
    block_builder::BlockBuilder,
    guest_mem_forget,
    mem_db::{AccountState, MemDb},
};

pub trait BlockBuildStrategy {
    type Db;
    type Output;

    fn build(block_builder: BlockBuilder<Self::Db>) -> Result<Self::Output>;
}

pub struct BuildFromMemDbStrategy {}

impl BuildFromMemDbStrategy {
    pub fn build_header(
        debug_storage_tries: &mut Option<HashMap<Address, MptNode>>,
        mut block_builder: BlockBuilder<MemDb>,
    ) -> Result<Header> {
        let db = block_builder.db.as_ref().unwrap();

        // apply state updates
        let state_trie = &mut block_builder.input.parent_state_trie;
        for (address, account) in &db.accounts {
            // if the account has not been touched, it can be ignored
            if account.state == AccountState::None {
                if let Some(map) = debug_storage_tries {
                    let storage_root = block_builder
                        .input
                        .parent_storage
                        .get(address)
                        .unwrap()
                        .0
                        .clone();
                    map.insert(*address, storage_root);
                }
                continue;
            }

            // compute the index of the current account in the state trie
            let state_trie_index = keccak(address);

            // remove deleted accounts from the state trie
            if account.state == AccountState::Deleted {
                state_trie.delete(&state_trie_index)?;
                continue;
            }

            // otherwise, compute the updated storage root for that account
            let state_storage = &account.storage;
            let storage_root = {
                // getting a mutable reference is more efficient than calling remove
                // every account must have an entry, even newly created accounts
                let (storage_trie, _) =
                    block_builder.input.parent_storage.get_mut(address).unwrap();
                // for cleared accounts always start from the empty trie
                if account.state == AccountState::StorageCleared {
                    storage_trie.clear();
                }

                // apply all new storage entries for the current account (address)
                for (key, value) in state_storage {
                    let storage_trie_index = keccak(key.to_be_bytes::<32>());
                    if value == &U256::ZERO {
                        storage_trie.delete(&storage_trie_index)?;
                    } else {
                        storage_trie.insert_rlp(&storage_trie_index, *value)?;
                    }
                }

                // insert the storage trie for host debugging
                if let Some(map) = debug_storage_tries {
                    map.insert(*address, storage_trie.clone());
                }

                storage_trie.hash()
            };

            let state_account = StateAccount {
                nonce: account.info.nonce,
                balance: account.info.balance,
                storage_root,
                code_hash: from_revm_b256(account.info.code_hash),
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }

        // update result header with the new state root
        let mut header = block_builder
            .header
            .take()
            .expect("Header was not initialized");
        header.state_root = state_trie.hash();

        // Leak memory, save cycles
        guest_mem_forget(block_builder);

        Ok(header)
    }
}

impl BlockBuildStrategy for BuildFromMemDbStrategy {
    type Db = MemDb;
    type Output = Header;

    #[inline(always)]
    fn build(block_builder: BlockBuilder<Self::Db>) -> Result<Self::Output> {
        BuildFromMemDbStrategy::build_header(&mut None, block_builder)
    }
}

pub struct DebugBuildFromMemDbStrategy {}

impl BlockBuildStrategy for DebugBuildFromMemDbStrategy {
    type Db = MemDb;
    type Output = (Header, HashMap<Address, MptNode>);

    fn build(block_builder: BlockBuilder<Self::Db>) -> Result<Self::Output> {
        let mut storage_trace = Some(Default::default());
        let header = BuildFromMemDbStrategy::build_header(&mut storage_trace, block_builder)?;
        Ok((header, storage_trace.unwrap()))
    }
}
