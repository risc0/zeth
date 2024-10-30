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

use alloy_consensus::Account;
use anyhow::Context;
use reth_consensus::Consensus;
use reth_ethereum_consensus::EthBeaconConsensus;
use reth_evm::execute::{
    BatchExecutor, BlockExecutionInput, BlockExecutorProvider, ExecutionOutcome,
};
use reth_evm_ethereum::execute::{EthBatchExecutor, EthExecutorProvider};
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::revm_primitives::alloy_primitives::Sealable;
use reth_primitives::{Block, Header, SealedHeader};
use reth_revm::db::states::StateChangeset;
use reth_revm::db::{BundleState, OriginalValuesKnown};
use reth_storage_errors::provider::ProviderError;
use std::fmt::Display;
use std::mem::take;
use zeth_core::db::{apply_changeset, MemoryDB};
use zeth_core::keccak::keccak;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::driver::RethDriver;
use zeth_core::stateless::execute::{DbExecutionInput, TransactionExecutionStrategy};
use zeth_core::stateless::finalize::{FinalizationStrategy, MPTFinalizationInput};
use zeth_core::stateless::initialize::MemoryDbStrategy;
use zeth_core::stateless::post_exec::PostExecutionValidationStrategy;
use zeth_core::stateless::pre_exec::{
    ConsensusPreExecValidationInput, PreExecutionValidationStrategy,
};

pub struct RethStatelessClient;

impl StatelessClient<Block, Header, MemoryDB, RethDriver> for RethStatelessClient {
    type Initialization = MemoryDbStrategy;
    type PreExecValidation = RethPreExecStrategy;
    type TransactionExecution = RethExecStrategy;
    type PostExecValidation = RethPostExecStrategy;
    type Finalization = RethFinalizationStrategy;
}

pub struct RethPreExecStrategy;

impl<Database> PreExecutionValidationStrategy<Block, Header, Database> for RethPreExecStrategy
where
    Database: 'static,
{
    type Input<'a> = ConsensusPreExecValidationInput<'a, Block, Header>;
    type Output<'b> = ();

    fn pre_execution_validation(
        (chain_spec, block, parent_header, total_difficulty): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output<'_>> {
        // Instantiate consensus engine
        let consensus = EthBeaconConsensus::new(chain_spec);
        // Validate total difficulty
        consensus
            .validate_header_with_total_difficulty(&block.header, *total_difficulty)
            .context("validate_header_with_total_difficulty")?;
        // Validate header (todo: seal beforehand to save rehashing costs)
        let sealed_block = take(block).seal_slow();
        consensus
            .validate_header(&sealed_block.header)
            .context("validate_header")?;
        // Validate header w.r.t. parent
        let sealed_parent_header = {
            let (parent_header, parent_header_seal) = take(parent_header).seal_slow().into_parts();
            SealedHeader::new(parent_header, parent_header_seal)
        };
        consensus
            .validate_header_against_parent(&sealed_block.header, &sealed_parent_header)
            .context("validate_header_against_parent")?;
        // Check pre-execution block conditions
        consensus
            .validate_block_pre_execution(&sealed_block)
            .context("validate_block_pre_execution")?;
        // Return values
        *block = sealed_block.unseal();
        *parent_header = sealed_parent_header.unseal();
        Ok(())
    }
}

pub struct RethExecStrategy;

impl<Database: reth_revm::Database> TransactionExecutionStrategy<Block, Header, Database>
    for RethExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = DbExecutionInput<'a, Block, Database>;
    type Output<'b> = EthBatchExecutor<EthEvmConfig, Database>;

    fn execute_transactions(
        (chain_spec, block, total_difficulty, db): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output<'_>> {
        // Instantiate execution engine using database
        let mut executor = EthExecutorProvider::ethereum(chain_spec.clone())
            .batch_executor(db.take().expect("Missing database."));
        // Execute transactions
        // let block_with_senders = BlockWithSenders {
        //     block,
        //     senders: vec![], // todo: recover signers with non-det hints
        // };
        let block_with_senders = take(block)
            .with_recovered_senders()
            .expect("Senders recovery failed");
        executor
            .execute_and_verify_one(BlockExecutionInput {
                block: &block_with_senders,
                total_difficulty: *total_difficulty,
            })
            .expect("Execution failed");

        *block = block_with_senders.block;
        Ok(executor)
    }
}

pub struct RethPostExecStrategy;

impl<Database: reth_revm::Database> PostExecutionValidationStrategy<Block, Header, Database>
    for RethPostExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = EthBatchExecutor<EthEvmConfig, Database>;
    type Output<'b> = BundleState;

    fn post_execution_validation(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>> {
        let ExecutionOutcome { bundle, .. } = input.finalize();
        Ok(bundle)
    }
}

pub struct RethFinalizationStrategy;

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
