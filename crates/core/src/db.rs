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

use crate::rescue::{Recoverable, Wrapper};
use alloy_primitives::map::HashMap;
use alloy_primitives::{Address, B256, U256};
use reth_primitives::revm_primitives::{Account, AccountInfo, Bytecode};
use reth_revm::db::states::StateChangeset;
use reth_revm::db::CacheDB;
use reth_revm::{Database, DatabaseCommit, DatabaseRef};
use reth_storage_errors::db::DatabaseError;

pub type MemoryDB = CacheDB<UnreachableDB>;

#[derive(Clone, Copy, Default)]
pub struct UnreachableDB;

impl DatabaseRef for UnreachableDB {
    type Error = DatabaseError;

    fn basic_ref(&self, _: Address) -> Result<Option<AccountInfo>, Self::Error> {
        unreachable!()
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, Self::Error> {
        unreachable!()
    }

    fn storage_ref(&self, _: Address, _: U256) -> Result<U256, Self::Error> {
        unreachable!()
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        unreachable!()
    }
}

impl Recoverable for MemoryDB {
    fn rescue(&mut self) -> Option<Self> {
        Some(core::mem::take(self))
    }
}

impl<DB: Database + Recoverable> Database for Wrapper<DB> {
    type Error = <DB as Database>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash(number)
    }
}

impl<DB: DatabaseRef + Recoverable> DatabaseRef for Wrapper<DB> {
    type Error = <DB as DatabaseRef>::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic_ref(address)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash_ref(code_hash)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage_ref(address, index)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash_ref(number)
    }
}

impl<DB: DatabaseCommit + Recoverable> DatabaseCommit for Wrapper<DB> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.inner.commit(changes)
    }
}

pub fn apply_changeset<DB>(
    db: &mut CacheDB<DB>,
    state_changeset: StateChangeset,
) -> anyhow::Result<()> {
    // Update account storages
    for storage in state_changeset.storage {
        let db_account = db.accounts.get_mut(&storage.address).unwrap();
        if storage.wipe_storage {
            db_account.storage.clear();
        }
        for (key, val) in storage.storage {
            db_account.storage.insert(key, val);
        }
    }
    // Update accounts in state trie
    for (address, account_info) in state_changeset.accounts {
        if account_info.is_none() {
            db.accounts.remove(&address);
            continue;
        }
        let db_account = db.accounts.get_mut(&address).unwrap();
        db_account.info = account_info.unwrap();
    }
    Ok(())
}
