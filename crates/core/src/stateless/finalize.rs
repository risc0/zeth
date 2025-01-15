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

use crate::db::memory::MemoryDB;
use crate::db::trie::TrieDB;
use crate::db::update::{into_plain_state, Update};
use crate::driver::CoreDriver;
use crate::keccak::keccak;
use crate::mpt::MptNode;
use crate::stateless::data::StorageEntry;
use alloy_consensus::Account;
use alloy_primitives::map::AddressHashMap;
use alloy_primitives::U256;
use anyhow::{bail, Context};
use reth_revm::db::states::StateChangeset;
use reth_revm::db::BundleState;

pub trait FinalizationStrategy<Driver: CoreDriver, Database> {
    fn finalize_state(
        block: &mut Driver::Block,
        state_trie: &mut MptNode,
        storage_tries: &mut AddressHashMap<StorageEntry>,
        parent_header: &mut Driver::Header,
        db: Option<&mut Database>,
        bundle_state: BundleState,
        with_further_updates: bool,
    ) -> anyhow::Result<()>;
}

pub struct TrieDbFinalizationStrategy;

impl<Driver: CoreDriver> FinalizationStrategy<Driver, TrieDB> for TrieDbFinalizationStrategy {
    fn finalize_state(
        block: &mut Driver::Block,
        _state_trie: &mut MptNode,
        _storage_tries: &mut AddressHashMap<StorageEntry>,
        parent_header: &mut Driver::Header,
        db: Option<&mut TrieDB>,
        bundle_state: BundleState,
        with_further_updates: bool,
    ) -> anyhow::Result<()> {
        let TrieDB {
            accounts: state_trie,
            storage: storage_tries,
            block_hashes,
            ..
        } = db.expect("Missing TrieDB instance");

        // Update the trie data
        <MemoryDbFinalizationStrategy as FinalizationStrategy<Driver, MemoryDB>>::finalize_state(
            block,
            state_trie,
            storage_tries,
            parent_header,
            None,
            bundle_state,
            false,
        )?;

        // Get the header
        let header = Driver::block_header(block);

        // Give back the tries
        if !with_further_updates {
            core::mem::swap(state_trie, _state_trie);
            core::mem::swap(storage_tries, _storage_tries);
        } else {
            block_hashes.insert(Driver::block_number(header), Driver::header_hash(header));
        }

        Ok(())
    }
}

pub struct MemoryDbFinalizationStrategy;

impl<Driver: CoreDriver> FinalizationStrategy<Driver, MemoryDB> for MemoryDbFinalizationStrategy {
    fn finalize_state(
        block: &mut Driver::Block,
        state_trie: &mut MptNode,
        storage_tries: &mut AddressHashMap<StorageEntry>,
        parent_header: &mut Driver::Header,
        db: Option<&mut MemoryDB>,
        bundle_state: BundleState,
        with_further_updates: bool,
    ) -> anyhow::Result<()> {
        // Apply state updates
        if state_trie.hash() != Driver::state_root(parent_header) {
            bail!(
                "Invalid state root (expected {:?}, got {:?})",
                Driver::state_root(parent_header),
                state_trie.hash()
            );
        }

        // Convert the state update bundle
        let state_changeset = into_plain_state(bundle_state);
        // Update the trie data
        let StateChangeset {
            accounts, storage, ..
        } = &state_changeset;
        // Apply storage trie changes
        for storage_change in storage {
            // getting a mutable reference is more efficient than calling remove
            // every account must have an entry, even newly created accounts
            let StorageEntry { storage_trie, .. } =
                storage_tries.get_mut(&storage_change.address).unwrap();
            // for cleared accounts always start from the empty trie
            if storage_change.wipe_storage {
                storage_trie.clear();
            }
            // apply all new storage entries for the current account (address)
            let mut deletions = Vec::with_capacity(storage_change.storage.len());
            for (key, value) in &storage_change.storage {
                let storage_trie_index = keccak(key.to_be_bytes::<32>());
                if value.is_zero() {
                    deletions.push(storage_trie_index);
                } else {
                    storage_trie
                        .insert_rlp(&storage_trie_index, value)
                        .context("storage_trie.insert_rlp")?;
                }
            }
            // Apply deferred storage trie deletions
            for storage_trie_index in deletions {
                storage_trie
                    .delete(&storage_trie_index)
                    .context("storage_trie.delete")?;
            }
        }
        // Apply account info + storage changes
        let mut deletions = Vec::with_capacity(accounts.len());
        for (address, account_info) in accounts {
            let state_trie_index = keccak(address);
            if account_info.is_none() {
                deletions.push(state_trie_index);
                continue;
            }
            let storage_root = {
                let StorageEntry { storage_trie, .. } = storage_tries.get(address).unwrap();
                storage_trie.hash()
            };

            let info = account_info.as_ref().unwrap();
            let state_account = Account {
                nonce: info.nonce,
                balance: info.balance,
                storage_root,
                code_hash: info.code_hash,
            };
            state_trie
                .insert_rlp(&state_trie_index, state_account)
                .context("state_trie.insert_rlp")?;
        }
        // Apply deferred state trie deletions
        for state_trie_index in deletions {
            state_trie
                .delete(&state_trie_index)
                .context("state_trie.delete")?;
        }
        // Apply account storage only changes
        for (address, StorageEntry { storage_trie, .. }) in storage_tries {
            if storage_trie.is_reference_cached() {
                continue;
            }
            let state_trie_index = keccak(address);
            let mut state_account = state_trie
                .get_rlp::<Account>(&state_trie_index)
                .context("state_trie.get_rlp")?
                .unwrap_or_default();
            let new_storage_root = storage_trie.hash();
            if state_account.storage_root != new_storage_root {
                state_account.storage_root = storage_trie.hash();
                state_trie
                    .insert_rlp(&state_trie_index, state_account)
                    .context("state_trie.insert_rlp (2)")?;
            }
        }

        // Validate final state trie
        let header = Driver::block_header(block);
        if Driver::state_root(header) != state_trie.hash() {
            bail!(
                "Invalid state root (expected {:?}, got {:?}) at block #{}",
                state_trie.hash(),
                Driver::state_root(header),
                Driver::block_number(header)
            );
        }

        // Update the database if available
        if with_further_updates {
            if let Some(db) = db {
                db.apply_changeset(state_changeset)?;

                db.insert_block_hash(
                    U256::from(Driver::block_number(header)),
                    Driver::header_hash(header),
                )?;
            }
        }

        Ok(())
    }
}
