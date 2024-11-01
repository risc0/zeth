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
use reth_primitives::KECCAK_EMPTY;
use reth_revm::db::states::{PlainStorageChangeset, StateChangeset};
use reth_revm::db::{BundleState, CacheDB};
use reth_revm::{Database, DatabaseCommit, DatabaseRef};
use reth_storage_errors::db::DatabaseError;

pub type MemoryDB = CacheDB<UnreachableDB>;

#[derive(Clone, Copy, Default)]
pub struct UnreachableDB;

impl DatabaseRef for UnreachableDB {
    type Error = DatabaseError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        unreachable!("basic_ref {address}")
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        unreachable!("code_by_hash_ref {code_hash}")
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        unreachable!("storage_ref {address}-{index}")
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        unreachable!("block_hash_ref {number}")
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
    // let mut code_omissions = Vec::new();
    for (address, account_info) in state_changeset.accounts {
        let Some(info) = account_info else {
            db.accounts.remove(&address);
            continue;
        };
        let db_account = db.accounts.get_mut(&address).unwrap();
        if info.code_hash != db_account.info.code_hash {
            // if info.code.is_none() && info.code_hash != KECCAK_EMPTY {
            //     code_omissions.push(address);
            // }
            db_account.info = info;
        } else {
            db_account.info.balance = info.balance;
            db_account.info.nonce = info.nonce;
        }
    }
    Ok(())
}

/// Copied from [BundleState::into_plane_state]. Modified to retain account code.
pub fn into_plain_state(bundle: BundleState) -> StateChangeset {
    // pessimistically pre-allocate assuming _all_ accounts changed.
    let state_len = bundle.state.len();
    let mut accounts = Vec::with_capacity(state_len);
    let mut storage = Vec::with_capacity(state_len);

    for (address, account) in bundle.state {
        // append account info if it is changed.
        let was_destroyed = account.was_destroyed();
        if account.is_info_changed() {
            accounts.push((address, account.info));
        }

        // append storage changes

        // NOTE: Assumption is that revert is going to remove whole plain storage from
        // database so we can check if plain state was wiped or not.
        let mut account_storage_changed = Vec::with_capacity(account.storage.len());

        for (key, slot) in account.storage {
            // If storage was destroyed that means that storage was wiped.
            // In that case we need to check if present storage value is different then ZERO.
            let destroyed_and_not_zero = was_destroyed && !slot.present_value.is_zero();

            // If account is not destroyed check if original values was changed,
            // so we can update it.
            let not_destroyed_and_changed = !was_destroyed && slot.is_changed();

            if destroyed_and_not_zero || not_destroyed_and_changed {
                account_storage_changed.push((key, slot.present_value));
            }
        }

        if !account_storage_changed.is_empty() || was_destroyed {
            // append storage changes to account.
            storage.push(PlainStorageChangeset {
                address,
                wipe_storage: was_destroyed,
                storage: account_storage_changed,
            });
        }
    }
    let contracts = bundle
        .contracts
        .into_iter()
        // remove empty bytecodes
        .filter(|(b, _)| *b != KECCAK_EMPTY)
        .collect::<Vec<_>>();
    StateChangeset {
        accounts,
        storage,
        contracts,
    }
}
