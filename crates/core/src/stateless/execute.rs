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

use crate::stateless::client::StatelessClientEngine;
use crate::stateless::data::StatelessClientData;
use alloy_consensus::Header;
use core::fmt::Display;
use core::mem::take;
use reth_evm::execute::{BatchExecutor, BlockExecutionInput, BlockExecutorProvider, ProviderError};
use reth_evm_ethereum::execute::{EthBatchExecutor, EthExecutorProvider};
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::Block;

pub trait TransactionExecutionStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output<'b>;
    fn execute_transactions(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>>;
}

pub struct RethExecStrategy;

impl<Database: reth_revm::Database> TransactionExecutionStrategy<Block, Header, Database>
    for RethExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = &'a mut StatelessClientEngine<Block, Header, Database>;
    type Output<'b> = EthBatchExecutor<EthEvmConfig, Database>;

    fn execute_transactions(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>> {
        // Unpack input
        let StatelessClientEngine {
            chain_spec,
            data:
                StatelessClientData {
                    block,
                    total_difficulty,
                    ..
                },
            db,
            ..
        } = input;
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
