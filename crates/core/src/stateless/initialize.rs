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
use crate::driver::CoreDriver;
use crate::keccak::keccak;
use crate::map::NoMapHasher;
use crate::mpt::MptNode;
use crate::stateless::data::StorageEntry;
use alloy_consensus::constants::EMPTY_ROOT_HASH;
use alloy_consensus::Account;
use alloy_primitives::map::{AddressHashMap, HashMap};
use alloy_primitives::{Bytes, B256, U256};
use anyhow::{bail, ensure};
use core::mem::take;
use reth_primitives::revm_primitives::Bytecode;
use reth_revm::db::{AccountState, DbAccount};
use reth_revm::primitives::AccountInfo;
use std::default::Default;

pub trait InitializationStrategy<Driver: CoreDriver, Database> {
    fn initialize_database(
        state_trie: &mut MptNode,
        storage_tries: &mut AddressHashMap<StorageEntry>,
        contracts: &mut Vec<Bytes>,
        parent_header: &mut Driver::Header,
        ancestor_headers: &mut Vec<Driver::Header>,
    ) -> anyhow::Result<Database>;
}

pub struct TrieDbInitializationStrategy;

impl<Driver: CoreDriver> InitializationStrategy<Driver, TrieDB> for TrieDbInitializationStrategy {
    fn initialize_database(
        state_trie: &mut MptNode,
        storage_tries: &mut AddressHashMap<StorageEntry>,
        contracts: &mut Vec<Bytes>,
        parent_header: &mut Driver::Header,
        ancestor_headers: &mut Vec<Driver::Header>,
    ) -> anyhow::Result<TrieDB> {
        // Verify starting state trie root
        if Driver::state_root(parent_header) != state_trie.hash() {
            bail!(
                "Invalid initial state trie: expected {}, got {}",
                Driver::state_root(parent_header),
                state_trie.hash()
            );
        }

        // hash all the contract code
        let contracts = take(contracts)
            .into_iter()
            .map(|bytes| (keccak(&bytes).into(), Bytecode::new_raw(bytes)))
            .collect();

        // Verify account data in db
        for (address, StorageEntry { storage_trie, .. }) in storage_tries.iter() {
            // load the account from the state trie
            let state_account = state_trie.get_rlp::<Account>(&keccak(address))?;

            // check that the account storage root matches the storage trie root of the input
            let storage_root = state_account.map_or(EMPTY_ROOT_HASH, |a| a.storage_root);
            if storage_root != storage_trie.hash() {
                bail!(
                    "Invalid storage trie for {}: expected {}, got {}",
                    address,
                    storage_root,
                    storage_trie.hash()
                )
            }
        }

        // prepare block hash history
        let mut block_hashes: HashMap<u64, B256, NoMapHasher> =
            HashMap::with_capacity_and_hasher(ancestor_headers.len() + 1, Default::default());
        block_hashes.insert(
            Driver::block_number(parent_header),
            Driver::header_hash(parent_header),
        );
        let mut prev = &*parent_header;
        for current in ancestor_headers.iter() {
            let current_hash = Driver::header_hash(current);
            if Driver::parent_hash(prev) != current_hash {
                bail!(
                    "Invalid chain: {} is not the parent of {}",
                    Driver::block_number(current),
                    Driver::block_number(prev)
                );
            }
            if Driver::block_number(parent_header) < Driver::block_number(current)
                || Driver::block_number(parent_header) - Driver::block_number(current) >= 256
            {
                bail!(
                    "Invalid chain: {} is not one of the {} most recent blocks",
                    Driver::block_number(current),
                    256,
                );
            }
            block_hashes.insert(Driver::block_number(current), current_hash);
            prev = current;
        }

        Ok(TrieDB {
            accounts: take(state_trie),
            storage: take(storage_tries),
            contracts,
            block_hashes,
        })
    }
}

pub struct MemoryDbInitializationStrategy;

impl<Driver: CoreDriver> InitializationStrategy<Driver, MemoryDB>
    for MemoryDbInitializationStrategy
{
    fn initialize_database(
        state_trie: &mut MptNode,
        storage_tries: &mut AddressHashMap<StorageEntry>,
        contracts: &mut Vec<Bytes>,
        parent_header: &mut Driver::Header,
        ancestor_headers: &mut Vec<Driver::Header>,
    ) -> anyhow::Result<MemoryDB> {
        // Verify starting state trie root
        if Driver::state_root(parent_header) != state_trie.hash() {
            bail!(
                "Invalid initial state trie: expected {}, got {}",
                Driver::state_root(parent_header),
                state_trie.hash()
            );
        }

        // hash all the contract code
        let contracts = take(contracts)
            .into_iter()
            .map(|bytes| (keccak(&bytes).into(), Bytecode::new_raw(bytes)))
            .collect();

        // Load account data into db
        let mut accounts =
            HashMap::with_capacity_and_hasher(storage_tries.len(), Default::default());
        for (
            address,
            StorageEntry {
                storage_trie,
                slots,
            },
        ) in storage_tries
        {
            // consume the slots, as they are no longer needed afterward
            let slots = take(slots);

            // load the account from the state trie
            let state_account = state_trie.get_rlp::<Account>(&keccak(address))?;

            // check that the account storage root matches the storage trie root of the input
            let storage_root = state_account.map_or(EMPTY_ROOT_HASH, |a| a.storage_root);
            ensure!(
                storage_root == storage_trie.hash(),
                "Invalid storage trie for {}: expected {}, got {}",
                address,
                storage_root,
                storage_trie.hash()
            );

            // load the account into memory
            let mem_account = match state_account {
                None => DbAccount::new_not_existing(),
                Some(state_account) => {
                    // load storage reads
                    let mut storage =
                        HashMap::with_capacity_and_hasher(slots.len(), Default::default());
                    for slot in slots {
                        let value: U256 = storage_trie
                            .get_rlp(&keccak(slot.to_be_bytes::<32>()))?
                            .unwrap_or_default();
                        storage.insert(slot, value);
                    }

                    DbAccount {
                        info: AccountInfo {
                            balance: state_account.balance,
                            nonce: state_account.nonce,
                            code_hash: state_account.code_hash,
                            code: None,
                        },
                        account_state: AccountState::None,
                        storage,
                    }
                }
            };

            accounts.insert(*address, mem_account);
        }

        // prepare block hash history
        let mut block_hashes: HashMap<U256, B256> =
            HashMap::with_capacity_and_hasher(ancestor_headers.len() + 1, Default::default());
        block_hashes.insert(
            U256::from(Driver::block_number(parent_header)),
            Driver::header_hash(parent_header),
        );
        let mut prev = &*parent_header;
        for current in ancestor_headers.iter() {
            let current_hash = Driver::header_hash(current);
            if Driver::parent_hash(prev) != current_hash {
                bail!(
                    "Invalid chain: {} is not the parent of {}",
                    Driver::block_number(current),
                    Driver::block_number(prev)
                );
            }
            if Driver::block_number(parent_header) < Driver::block_number(current)
                || Driver::block_number(parent_header) - Driver::block_number(current) >= 256
            {
                bail!(
                    "Invalid chain: {} is not one of the {} most recent blocks",
                    Driver::block_number(current),
                    256,
                );
            }
            block_hashes.insert(U256::from(Driver::block_number(current)), current_hash);
            prev = current;
        }

        // Initialize database
        Ok(MemoryDB {
            accounts,
            contracts,
            block_hashes,
            ..Default::default()
        })
    }
}
