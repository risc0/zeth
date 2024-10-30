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
use reth_optimism_evm::OpExecutorProvider;
use reth_primitives::revm_primitives::alloy_primitives::Sealable;
use reth_primitives::{Block, Header, SealedHeader};
use reth_revm::db::BundleState;
use reth_storage_errors::provider::ProviderError;
use std::fmt::Display;
use std::mem::take;
use zeth_core::stateless::execute::{DbExecutionInput, TransactionExecutionStrategy};
use zeth_core::stateless::pre_exec::{
    ConsensusPreExecValidationInput, PreExecutionValidationStrategy,
};

pub struct OpRethPreExecStrategy;

impl<Database: 'static> PreExecutionValidationStrategy<Block, Header, Database>
    for OpRethPreExecStrategy
{
    type Input<'a> = ConsensusPreExecValidationInput<'a, OpChainSpec, Block, Header>;
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

impl<Database: reth_revm::Database> TransactionExecutionStrategy<Block, Header, Database>
    for OpRethExecStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    type Input<'a> = DbExecutionInput<'a, OpChainSpec, Block, Database>;

    fn execute_transactions(
        (chain_spec, block, total_difficulty, db): Self::Input<'_>,
    ) -> anyhow::Result<BundleState> {
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
        // Return block
        *block = block_with_senders.block;
        // Return bundle state
        let ExecutionOutcome { bundle, .. } = executor.finalize();
        Ok(bundle)
    }
}

pub struct OpRethStatelessClient;

// impl StatelessClient<OpChainSpec, Block, Header, MemoryDB, RethDriver> for OpRethStatelessClient {
//     type Initialization = MemoryDbStrategy;
//     type PreExecValidation = OpRethPreExecStrategy;
//     type TransactionExecution = OpRethExecStrategy;
//     type Finalization = RethFinalizationStrategy;
// }
