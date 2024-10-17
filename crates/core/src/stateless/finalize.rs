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
use crate::stateless::block::StatelessClientBlock;
use crate::stateless::client::StatelessClientEngine;
use alloy_consensus::{Account, Header};
use alloy_primitives::B256;
use anyhow::bail;
use core::fmt::Display;
use core::mem::take;
use reth_evm::execute::ProviderError;
use reth_primitives::Block;
use reth_revm::db::BundleState;

pub trait FinalizationStrategy<Block, Header, Database> {
    type Input;

    type Output;

    fn finalize(
        stateless_client_engine: &mut StatelessClientEngine<Block, Header, Database>,
        state_delta: Self::Input,
    ) -> anyhow::Result<Self::Output>;
}

pub struct RethFinalizationStrategy;

impl<Database: reth_revm::Database> FinalizationStrategy<Block, Header, Database>
    for RethFinalizationStrategy
where
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input = BundleState;
    type Output = B256;

    fn finalize(
        stateless_client_engine: &mut StatelessClientEngine<Block, Header, Database>,
        state_delta: Self::Input,
    ) -> anyhow::Result<Self::Output> {
        let StatelessClientEngine {
            block:
                StatelessClientBlock {
                    block,
                    parent_state_trie,
                    parent_storage,
                    ..
                },
            ..
        } = stateless_client_engine;
        // Apply state updates
        let mut state_trie = take(parent_state_trie);
        for (address, account) in state_delta.state {
            // if the account has not been touched, it can be ignored
            if account.status.is_not_modified() {
                continue;
            }
            // compute the index of the current account in the state trie
            let state_trie_index = keccak(address);
            // remove deleted accounts from the state trie
            if account.info.is_none() {
                state_trie.delete(&state_trie_index)?;
                continue;
            }
            // otherwise, compute the updated storage root for that account
            let state_storage = &account.storage;
            let storage_root = {
                // getting a mutable reference is more efficient than calling remove
                // every account must have an entry, even newly created accounts
                let (storage_trie, _) = parent_storage.get_mut(&address).unwrap();
                // for cleared accounts always start from the empty trie
                let is_storage_cleared = account.was_destroyed();
                if is_storage_cleared {
                    storage_trie.clear();
                }
                // apply all new storage entries for the current account (address)
                for (key, slot) in state_storage {
                    if slot.present_value.is_zero() && is_storage_cleared {
                        continue;
                    }
                    let storage_trie_index = keccak(key.to_be_bytes::<32>());
                    if slot.present_value.is_zero() {
                        storage_trie.delete(&storage_trie_index)?;
                    } else {
                        storage_trie.insert_rlp(&storage_trie_index, slot.present_value)?;
                    }
                }
                storage_trie.hash()
            };

            let info = account.info.unwrap();
            let state_account = Account {
                nonce: info.nonce,
                balance: info.balance,
                storage_root,
                code_hash: info.code_hash,
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }
        // Validate final state trie
        if block.header.state_root != state_trie.hash() {
            bail!("Unexpected state root");
        }

        Ok(block.hash_slow())
    }
}
