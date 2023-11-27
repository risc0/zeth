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

use core::mem;

use anyhow::Result;
use revm::{Database, DatabaseCommit};
use zeth_primitives::{
    block::Header,
    keccak::keccak,
    transactions::TxEssence,
    trie::{MptNode, StateAccount},
    U256,
};

use crate::{
    builder::BlockBuilder,
    guest_mem_forget,
    mem_db::{AccountState, MemDb},
};

pub trait BlockFinalizeStrategy<D>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
{
    fn finalize<E>(block_builder: BlockBuilder<D, E>) -> Result<(Header, MptNode)>
    where
        E: TxEssence;
}

pub struct MemDbBlockFinalizeStrategy {}

impl BlockFinalizeStrategy<MemDb> for MemDbBlockFinalizeStrategy {
    fn finalize<E: TxEssence>(
        mut block_builder: BlockBuilder<MemDb, E>,
    ) -> Result<(Header, MptNode)> {
        let db = block_builder.db.take().expect("DB not initialized");

        // apply state updates
        let mut state_trie = mem::take(&mut block_builder.input.parent_state_trie);
        for (address, account) in &db.accounts {
            // if the account has not been touched, it can be ignored
            if account.state == AccountState::None {
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

                storage_trie.hash()
            };

            let state_account = StateAccount {
                nonce: account.info.nonce,
                balance: account.info.balance,
                storage_root,
                code_hash: account.info.code_hash,
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }

        // update result header with the new state root
        let mut header = block_builder.header.take().expect("Header not initialized");
        header.state_root = state_trie.hash();

        // Leak memory, save cycles
        guest_mem_forget(block_builder);

        Ok((header, state_trie))
    }
}
