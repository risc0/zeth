// Copyright 2024, 2025 RISC Zero, Inc.
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

use crate::db::unreachable::UnreachableDB;
use crate::db::update::Update;
use crate::rescue::Recoverable;
use alloy_primitives::{B256, U256};
use reth_revm::db::states::{PlainStorageChangeset, StateChangeset};
use reth_revm::db::{AccountState, CacheDB};

pub type MemoryDB = CacheDB<UnreachableDB>;

impl<DB: Default> Recoverable for CacheDB<DB> {
    fn rescue(&mut self) -> Option<Self> {
        Some(core::mem::take(self))
    }
}

impl<DB> Update for CacheDB<DB> {
    fn apply_changeset(&mut self, changeset: StateChangeset) -> anyhow::Result<()> {
        // Update accounts in state trie
        for (address, account_info) in changeset.accounts {
            let db_account = self.accounts.get_mut(&address).unwrap();
            // Reset the account state
            db_account.account_state = AccountState::None;
            // Update account info
            db_account.info = account_info.unwrap_or_default();
        }
        // Update account storages
        for PlainStorageChangeset {
            address,
            wipe_storage,
            storage,
        } in changeset.storage
        {
            let db_account = self.accounts.get_mut(&address).unwrap();
            db_account.account_state = AccountState::None;
            if wipe_storage {
                db_account.storage.clear();
            }
            for (key, val) in storage {
                db_account.storage.insert(key, val);
            }
        }
        Ok(())
    }
    fn insert_block_hash(&mut self, block_number: U256, block_hash: B256) -> anyhow::Result<()> {
        self.block_hashes.insert(block_number, block_hash);
        Ok(())
    }
}
