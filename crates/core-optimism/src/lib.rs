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

use anyhow::Context;
use reth_consensus::Consensus;
use reth_evm::execute::{
    BatchExecutor, BlockExecutionInput, BlockExecutorProvider, ExecutionOutcome,
};
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_consensus::OptimismBeaconConsensus;
use reth_optimism_evm::{OpBatchExecutor, OpExecutorProvider, OptimismEvmConfig};
use reth_primitives::revm_primitives::alloy_primitives::Sealable;
use reth_primitives::{Block, Header, SealedHeader};
use reth_revm::db::BundleState;
use reth_revm::primitives::U256;
use reth_storage_errors::provider::ProviderError;
use std::fmt::Display;
use std::mem::take;
use std::sync::Arc;
use zeth_core::stateless::execute::TransactionExecutionStrategy;
use zeth_core::stateless::post_exec::PostExecutionValidationStrategy;
use zeth_core::stateless::pre_exec::PreExecutionValidationStrategy;

pub struct OpRethPreExecStrategy;

pub type OpRethPreExecValidationInput<'a, B, H> =
    (Arc<OpChainSpec>, &'a mut B, &'a mut H, &'a mut U256);

impl<Database: 'static> PreExecutionValidationStrategy<Block, Header, Database>
    for OpRethPreExecStrategy
{
    type Input<'a> = OpRethPreExecValidationInput<'a, Block, Header>;
    type Output<'b> = ();

    fn pre_execution_validation(
        (chain_spec, block, parent_header, total_difficulty): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output<'_>> {
        // Instantiate consensus engine
        let consensus = OptimismBeaconConsensus::new(chain_spec);
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

pub struct OpRethExecStrategy;

pub type OpDbExecutionInput<'a, B, D> =
    (Arc<OpChainSpec>, &'a mut B, &'a mut U256, &'a mut Option<D>);

impl<Database: reth_revm::Database> TransactionExecutionStrategy<Block, Header, Database>
    for OpRethExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = OpDbExecutionInput<'a, Block, Database>;
    type Output<'b> = OpBatchExecutor<OptimismEvmConfig, Database>;

    fn execute_transactions(
        (chain_spec, block, total_difficulty, db): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output<'_>> {
        // Instantiate execution engine using database
        let mut executor = OpExecutorProvider::optimism(chain_spec.clone())
            .batch_executor(db.take().expect("Missing database"));
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

pub struct OpRethPostExecStrategy;

impl<Database: reth_revm::Database> PostExecutionValidationStrategy<Block, Header, Database>
    for OpRethPostExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = OpBatchExecutor<OptimismEvmConfig, Database>;
    type Output<'b> = BundleState;

    fn post_execution_validation(executor: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>> {
        let ExecutionOutcome { bundle, .. } = executor.finalize();
        Ok(bundle)
    }
}
