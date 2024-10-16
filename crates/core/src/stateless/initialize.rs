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

use crate::keccak::keccak;
use crate::stateless::client::StatelessClientEngine;
use alloy_consensus::Account;
use alloy_primitives::map::HashMap;
use alloy_primitives::{Bytes, B256, U256};
use anyhow::bail;
use core::mem::take;
use reth_primitives::revm_primitives::Bytecode;
use reth_primitives::{Header, KECCAK_EMPTY};
use reth_revm::db::{AccountState, DbAccount};
use reth_revm::primitives::AccountInfo;
use reth_revm::InMemoryDB;

pub trait InitializationStrategy<Block, Header, Database> {
    fn initialize_database(
        stateless_client_engine: StatelessClientEngine<Block, Header, Database>,
    ) -> anyhow::Result<StatelessClientEngine<Block, Header, Database>>;
}

pub struct InMemoryDbStrategy;

impl<Block> InitializationStrategy<Block, Header, InMemoryDB> for InMemoryDbStrategy {
    fn initialize_database(
        mut stateless_client_engine: StatelessClientEngine<Block, Header, InMemoryDB>,
    ) -> anyhow::Result<StatelessClientEngine<Block, Header, InMemoryDB>> {
        // Verify starting state trie root
        if stateless_client_engine.block.parent_header.state_root
            != stateless_client_engine.block.parent_state_trie.hash()
        {
            bail!(
                "Invalid state trie: expected {}, got {}",
                stateless_client_engine.block.parent_header.state_root,
                stateless_client_engine.block.parent_state_trie.hash()
            );
        }

        // hash all the contract code
        let contracts: HashMap<B256, Bytes> = take(&mut stateless_client_engine.block.contracts)
            .into_iter()
            .map(|bytes| (keccak(&bytes).into(), bytes))
            .collect();

        // Load account data into db
        let mut accounts =
            HashMap::with_capacity(stateless_client_engine.block.parent_storage.len());
        for (address, (storage_trie, slots)) in &mut stateless_client_engine.block.parent_storage {
            // consume the slots, as they are no longer needed afterward
            let slots = take(slots);

            // load the account from the state trie or empty if it does not exist
            let state_account = stateless_client_engine
                .block
                .parent_state_trie
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

            // load the corresponding code
            let code_hash = state_account.code_hash;
            let bytecode = if code_hash.0 == KECCAK_EMPTY.0 {
                Bytecode::new()
            } else {
                let bytes = contracts.get(&code_hash).unwrap().clone();
                Bytecode::new_raw(bytes)
            };

            // load storage reads
            let mut storage = HashMap::with_capacity(slots.len());
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
                    code: Some(bytecode),
                },
                account_state: AccountState::None,
                storage,
            };

            accounts.insert(*address, mem_account);
        }

        // prepare block hash history
        let mut block_hashes: HashMap<U256, B256> =
            HashMap::with_capacity(stateless_client_engine.block.ancestor_headers.len() + 1);
        block_hashes.insert(
            U256::from(stateless_client_engine.block.parent_header.number),
            stateless_client_engine.block.parent_header.hash_slow(),
        );
        let mut prev = &stateless_client_engine.block.parent_header;
        for current in &stateless_client_engine.block.ancestor_headers {
            let current_hash = current.hash_slow();
            if prev.parent_hash != current_hash {
                bail!(
                    "Invalid chain: {} is not the parent of {}",
                    current.number,
                    prev.number
                );
            }
            if stateless_client_engine.block.parent_header.number < current.number
                || stateless_client_engine.block.parent_header.number - current.number >= 256
            {
                bail!(
                    "Invalid chain: {} is not one of the {} most recent blocks",
                    current.number,
                    256,
                );
            }
            block_hashes.insert(U256::from(current.number), current_hash);
            prev = current;
        }

        // Store database
        let mut db = InMemoryDB::default();
        db.accounts = accounts;
        db.block_hashes = block_hashes;
        stateless_client_engine.db = Some(db);
        Ok(stateless_client_engine)
    }
}
