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

use crate::stateless::block::StatelessClientBlock;
use crate::stateless::client::StatelessClientEngine;
use alloy_consensus::Header;
use reth_chainspec::MAINNET;
use reth_evm::execute::{BatchExecutor, BlockExecutionInput, BlockExecutorProvider};
use reth_evm_ethereum::execute::EthExecutorProvider;
use reth_primitives::{Block, BlockWithSenders};
use reth_revm::Database;

pub trait TransactionExecutionStrategy<Block, Header, Database> {
    fn execute_transactions(
        stateless_client_engine: StatelessClientEngine<Block, Header, Database>,
    ) -> anyhow::Result<StatelessClientEngine<Block, Header, Database>>;
}

pub struct RethExecStrategy;

impl<Database: Database> TransactionExecutionStrategy<Block, Header, Database>
    for RethExecStrategy
{
    fn execute_transactions(
        stateless_client_engine: StatelessClientEngine<Block, Header, Database>,
    ) -> anyhow::Result<StatelessClientEngine<Block, Header, Database>> {
        // Unpack client instance
        let StatelessClientEngine {
            chain_spec,
            block:
                StatelessClientBlock {
                    block,
                    parent_state_trie,
                    parent_storage,
                    contracts,
                    parent_header,
                    ancestor_headers,
                },
            total_difficulty,
            db,
            db_rescue,
        } = stateless_client_engine;
        // Instantiate execution engine
        let mut executor = EthExecutorProvider::ethereum(chain_spec.clone()).batch_executor(db);
        // Execute transactions
        let block_with_senders = BlockWithSenders {
            block,
            senders: vec![], // todo: recover signers with non-det hints
        };
        executor
            .execute_and_verify_one(BlockExecutionInput {
                block: &block_with_senders,
                total_difficulty,
            })
            .expect("Execution failed");

        let outcome = executor.finalize();

        // Repack client values
        Ok(StatelessClientEngine {
            chain_spec,
            block: StatelessClientBlock {
                block: block_with_senders.block,
                parent_state_trie,
                parent_storage,
                contracts,
                parent_header,
                ancestor_headers,
            },
            total_difficulty,
            db,
            db_rescue,
        })
    }
}
