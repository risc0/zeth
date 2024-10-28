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

use crate::db::{apply_changeset, MemoryDB};
use crate::keccak::keccak;
use crate::mpt::MptNode;
use crate::stateless::data::StorageEntry;
use alloy_consensus::{Account, Header};
use alloy_primitives::map::HashMap;
use alloy_primitives::Address;
use anyhow::Context;
use reth_primitives::Block;
use reth_revm::db::states::StateChangeset;
use reth_revm::db::{BundleState, OriginalValuesKnown};

pub trait FinalizationStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output;

    fn finalize(input: Self::Input<'_>) -> anyhow::Result<Self::Output>;
}

pub struct RethFinalizationStrategy;
pub type MPTFinalizationInput<'a, B, H, D> = (
    &'a mut B,
    &'a mut MptNode,
    &'a mut HashMap<Address, StorageEntry>,
    &'a mut H,
    Option<&'a mut D>,
    BundleState,
);

impl FinalizationStrategy<Block, Header, MemoryDB> for RethFinalizationStrategy {
    type Input<'a> = MPTFinalizationInput<'a, Block, Header, MemoryDB>;
    type Output = ();

    fn finalize(
        (block, state_trie, storage_tries, parent_header, db, bundle_state): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output> {
        // Apply state updates
        assert_eq!(state_trie.hash(), parent_header.state_root);

        let state_changeset = bundle_state.into_plain_state(OriginalValuesKnown::Yes);

        // Update the trie data
        let StateChangeset {
            accounts, storage, ..
        } = &state_changeset;
        // Apply storage trie changes
        for storage_change in storage {
            // getting a mutable reference is more efficient than calling remove
            // every account must have an entry, even newly created accounts
            let (storage_trie, _) = storage_tries.get_mut(&storage_change.address).unwrap();
            // for cleared accounts always start from the empty trie
            if storage_change.wipe_storage {
                storage_trie.clear();
            }
            // apply all new storage entries for the current account (address)
            for (key, value) in &storage_change.storage {
                let storage_trie_index = keccak(key.to_be_bytes::<32>());
                if value.is_zero() {
                    storage_trie
                        .delete(&storage_trie_index)
                        .context("storage_trie.delete")?;
                } else {
                    storage_trie
                        .insert_rlp(&storage_trie_index, value)
                        .context("storage_trie.insert_rlp")?;
                }
            }
        }
        // Apply account info + storage changes
        for (address, account_info) in accounts {
            let state_trie_index = keccak(address);
            if account_info.is_none() {
                state_trie
                    .delete(&state_trie_index)
                    .context("state_trie.delete")?;
                continue;
            }
            let storage_root = {
                let (storage_trie, _) = storage_tries.get(address).unwrap();
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
        // Apply account storage only changes
        for (address, (storage_trie, _)) in storage_tries {
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

        // Update the database
        if let Some(db) = db {
            apply_changeset(db, state_changeset)?;
        }

        // Validate final state trie
        assert_eq!(block.header.state_root, state_trie.hash());

        Ok(())
    }
}