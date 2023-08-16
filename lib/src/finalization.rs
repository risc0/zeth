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
use revm::{
    db::AccountState,
    primitives::{Address, U256},
};
use zeth_primitives::{
    block::Header,
    guest_mem_forget,
    keccak::keccak,
    revm::from_revm_b256,
    trie::{MptNode, TrieAccount},
};

use crate::{auth_db::CachedAuthDb, block_builder::BlockBuilder};

pub trait BlockBuildStrategy {
    type Db;

    fn build(&mut self, block_builder: BlockBuilder<Self::Db>) -> Result<Header>;
}

pub struct BuildFromCachedAuthDbStrategy {
    debug_storage_tries: Option<HashMap<Address, MptNode>>,
}

impl BuildFromCachedAuthDbStrategy {
    pub fn without_debugging() -> Self {
        Self {
            debug_storage_tries: None,
        }
    }

    pub fn with_debugging() -> Self {
        Self {
            debug_storage_tries: Some(Default::default()),
        }
    }

    pub fn take_storage_trace(self) -> Option<HashMap<Address, MptNode>> {
        self.debug_storage_tries
    }
}

impl BlockBuildStrategy for BuildFromCachedAuthDbStrategy {
    type Db = CachedAuthDb;

    fn build(&mut self, mut block_builder: BlockBuilder<Self::Db>) -> Result<Header> {
        let mut cached_db = block_builder.db.take().unwrap();

        // apply state updates
        let state_trie = &mut cached_db.db.state_trie;
        for (address, account) in cached_db.accounts.iter() {
            // if the account has not been touched, it can be ignored
            if account.account_state == AccountState::None {
                // store the root node for debugging
                if let Some(map) = &mut self.debug_storage_tries {
                    let storage_root = cached_db.db.storage_tries.get(address).unwrap().clone();
                    map.insert(*address, storage_root);
                }
                continue;
            }

            // compute the index of the current account in the state trie
            let state_trie_index = keccak(address);

            // remove deleted accounts from the state trie
            if account.info.is_empty() {
                state_trie.delete(&state_trie_index)?;
                continue;
            }

            // otherwise, compute the updated storage root for that account

            // getting a mutable reference is more efficient than calling remove
            // every account must have an entry, even newly created accounts
            let storage_trie = cached_db.db.storage_tries.get_mut(address).unwrap();
            // for cleared accounts always start from the empty trie
            if account.account_state == AccountState::StorageCleared {
                storage_trie.clear();
            }

            // apply all new storage entries for the current account (address)
            for (key, value) in &account.storage {
                let storage_trie_index = keccak(key.to_be_bytes::<32>());
                if value == &U256::ZERO {
                    storage_trie.delete(&storage_trie_index)?;
                } else {
                    storage_trie.insert_rlp(&storage_trie_index, *value)?;
                }
            }

            // insert the storage trie for host debugging
            if let Some(map) = &mut self.debug_storage_tries {
                map.insert(*address, storage_trie.clone());
            }

            let state_account = TrieAccount {
                nonce: account.info.nonce,
                balance: account.info.balance,
                storage_root: storage_trie.hash(),
                code_hash: from_revm_b256(account.info.code_hash),
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }

        // update result header with the new state root
        let mut header = block_builder
            .header
            .take()
            // .expect("Header was not initialized");
            .unwrap();
        header.state_root = state_trie.hash();

        // Leak memory, save cycles
        guest_mem_forget(block_builder);
        guest_mem_forget(cached_db);

        Ok(header)
    }
}
