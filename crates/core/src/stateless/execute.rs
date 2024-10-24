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

use alloy_consensus::Header;
use alloy_primitives::U256;
use core::fmt::Display;
use core::mem::take;
use reth_chainspec::ChainSpec;
use reth_evm::execute::{BatchExecutor, BlockExecutionInput, BlockExecutorProvider, ProviderError};
use reth_evm_ethereum::execute::{EthBatchExecutor, EthExecutorProvider};
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::Block;
use std::sync::Arc;

pub trait TransactionExecutionStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output<'b>;
    fn execute_transactions(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>>;
}

pub struct RethExecStrategy;
pub type DbExecutionInput<'a, B, D> = (Arc<ChainSpec>, &'a mut B, &'a mut U256, &'a mut Option<D>);

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
