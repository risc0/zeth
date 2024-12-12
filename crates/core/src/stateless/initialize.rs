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

use crate::db::MemoryDB;
use crate::driver::CoreDriver;
use crate::keccak::keccak;
use crate::mpt::MptNode;
use crate::stateless::data::StorageEntry;
use alloy_consensus::Account;
use alloy_primitives::map::HashMap;
use alloy_primitives::{Address, Bytes, B256, U256};
use anyhow::bail;
use core::mem::take;
use reth_primitives::revm_primitives::Bytecode;
use reth_revm::db::{AccountState, DbAccount};
use reth_revm::primitives::AccountInfo;
use std::default::Default;

pub trait InitializationStrategy<Driver: CoreDriver, Database> {
    fn initialize_database(
        state_trie: &mut MptNode,
        storage_tries: &mut HashMap<Address, StorageEntry>,
        contracts: &mut Vec<Bytes>,
        parent_header: &mut Driver::Header,
        ancestor_headers: &mut Vec<Driver::Header>,
    ) -> anyhow::Result<Database>;
}

pub struct MemoryDbStrategy;

impl<Driver: CoreDriver> InitializationStrategy<Driver, MemoryDB> for MemoryDbStrategy {
    fn initialize_database(
        state_trie: &mut MptNode,
        storage_tries: &mut HashMap<Address, StorageEntry>,
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
        for (address, (storage_trie, slots)) in storage_tries {
            // consume the slots, as they are no longer needed afterward
            let slots = take(slots);

            // load the account from the state trie or empty if it does not exist
            let state_account = state_trie
                .get_rlp::<Account>(&keccak(address))?
                .unwrap_or_default();
            // Verify storage trie root
            if storage_trie.hash() != state_account.storage_root {
                bail!(
                    "Invalid storage trie for {:?}: expected {}, got {}",
                    address,
                    state_account.storage_root,
                    storage_trie.hash()
                );
            }

            // load storage reads
            let mut storage = HashMap::with_capacity_and_hasher(slots.len(), Default::default());
            for slot in slots {
                let value: U256 = storage_trie
                    .get_rlp(&keccak(slot.to_be_bytes::<32>()))?
                    .unwrap_or_default();
                storage.insert(slot, value);
            }

            let mem_account = DbAccount {
                info: AccountInfo {
                    balance: state_account.balance,
                    nonce: state_account.nonce,
                    code_hash: state_account.code_hash,
                    code: None,
                },
                account_state: AccountState::None,
                storage,
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
