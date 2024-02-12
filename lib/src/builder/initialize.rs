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

use core::mem;

use anyhow::{bail, Result};
use hashbrown::HashMap;
use revm::{
    primitives::{AccountInfo, Bytecode, B256},
    Database, DatabaseCommit,
};
use zeth_primitives::{
    keccak::{keccak, KECCAK_EMPTY},
    transactions::TxEssence,
    trie::StateAccount,
    Bytes,
};

use crate::{
    builder::BlockBuilder,
    consts::MAX_BLOCK_HASH_AGE,
    guest_mem_forget,
    mem_db::{AccountState, DbAccount, MemDb},
};

pub trait DbInitStrategy<D>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
{
    fn initialize_database<E>(block_builder: BlockBuilder<D, E>) -> Result<BlockBuilder<D, E>>
    where
        E: TxEssence;
}

pub struct MemDbInitStrategy {}

impl DbInitStrategy<MemDb> for MemDbInitStrategy {
    fn initialize_database<E: TxEssence>(
        mut block_builder: BlockBuilder<MemDb, E>,
    ) -> Result<BlockBuilder<MemDb, E>> {
        // Verify state trie root
        if block_builder.input.parent_state_trie.hash()
            != block_builder.input.state_input.parent_header.state_root
        {
            bail!(
                "Invalid state trie: expected {}, got {}",
                block_builder.input.state_input.parent_header.state_root,
                block_builder.input.parent_state_trie.hash()
            );
        }

        // hash all the contract code
        let contracts: HashMap<B256, Bytes> = mem::take(&mut block_builder.input.contracts)
            .into_iter()
            .map(|bytes| (keccak(&bytes).into(), bytes))
            .collect();

        // Load account data into db
        let mut accounts = HashMap::with_capacity(block_builder.input.parent_storage.len());
        for (address, (storage_trie, slots)) in &mut block_builder.input.parent_storage {
            // consume the slots, as they are no longer needed afterwards
            let slots = mem::take(slots);

            // load the account from the state trie or empty if it does not exist
            let state_account = block_builder
                .input
                .parent_state_trie
                .get_rlp::<StateAccount>(&keccak(address))?
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
                let value: zeth_primitives::U256 = storage_trie
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
                state: AccountState::None,
                storage,
            };

            accounts.insert(*address, mem_account);
        }
        guest_mem_forget(contracts);

        // prepare block hash history
        let mut block_hashes =
            HashMap::with_capacity(block_builder.input.ancestor_headers.len() + 1);
        block_hashes.insert(
            block_builder.input.state_input.parent_header.number,
            block_builder.input.state_input.parent_header.hash(),
        );
        let mut prev = &block_builder.input.state_input.parent_header;
        for current in &block_builder.input.ancestor_headers {
            let current_hash = current.hash();
            if prev.parent_hash != current_hash {
                bail!(
                    "Invalid chain: {} is not the parent of {}",
                    current.number,
                    prev.number
                );
            }
            if block_builder.input.state_input.parent_header.number < current.number
                || block_builder.input.state_input.parent_header.number - current.number
                    >= MAX_BLOCK_HASH_AGE
            {
                bail!(
                    "Invalid chain: {} is not one of the {} most recent blocks",
                    current.number,
                    MAX_BLOCK_HASH_AGE,
                );
            }
            block_hashes.insert(current.number, current_hash);
            prev = current;
        }

        // Store database
        Ok(block_builder.with_db(MemDb {
            accounts,
            block_hashes,
        }))
    }
}
