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
use crate::stateless::data::StatelessClientData;
use alloy_consensus::{Account, BlockHeader, Header};
use alloy_primitives::{B256, U256};
use anyhow::{bail, Context};
use core::fmt::Display;
use core::mem::take;
use reth_evm::execute::ProviderError;
use reth_primitives::Block;
use reth_revm::db::states::StateChangeset;
use reth_revm::db::{BundleState, OriginalValuesKnown};

pub trait FinalizationStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output;

    fn finalize(input: Self::Input<'_>) -> anyhow::Result<Self::Output>;
}

pub struct RethFinalizationStrategy;
pub type EngineFinalizationStrategyInput<'a, B, H, D> = (
    &'a mut StatelessClientEngine<B, H, D>,
    BundleState,
);

impl<Database: reth_revm::Database> FinalizationStrategy<Block, Header, Database>
    for RethFinalizationStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = EngineFinalizationStrategyInput<'a, Block, Header, Database>;
    type Output = (B256, U256, U256);

    fn finalize(input: Self::Input<'_>) -> anyhow::Result<Self::Output> {
        // Unpack input
        let (
            StatelessClientEngine {
                data:
                    StatelessClientData {
                        block,
                        parent_state_trie,
                        parent_storage,
                        parent_header,
                        total_difficulty,
                        ..
                    },
                ..
            },
            state_delta,
        ) = input;
        // Apply state updates
        let mut state_trie = take(parent_state_trie);
        assert_eq!(state_trie.hash(), parent_header.state_root);

        let StateChangeset {
            accounts, storage, ..
        } = state_delta.into_plain_state(OriginalValuesKnown::Yes);
        // Apply storage trie changes
        for storage_change in storage.into_iter() {
            // getting a mutable reference is more efficient than calling remove
            // every account must have an entry, even newly created accounts
            let (storage_trie, _) = parent_storage.get_mut(&storage_change.address).unwrap();
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
        // Apply account info changes
        for (address, account_info) in accounts.into_iter() {
            let state_trie_index = keccak(address);
            if account_info.is_none() {
                state_trie
                    .delete(&state_trie_index)
                    .context("state_trie.delete")?;
                continue;
            }
            let storage_root = {
                let (storage_trie, _) = parent_storage.remove(&address).unwrap();
                storage_trie.hash()
            };

            let info = account_info.unwrap();
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
        // Apply storage root updates
        for (address, (storage_trie, _)) in parent_storage {
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
        if block.header.state_root != state_trie.hash() {
            bail!(
                "Unexpected final state root! Found {} but expected {}",
                state_trie.hash(),
                block.header.state_root,
            );
        }
        // Add difficulty
        let total_difficulty = *total_difficulty + block.header.difficulty();
        Ok((block.hash_slow(), total_difficulty, U256::from(1)))
    }
}
