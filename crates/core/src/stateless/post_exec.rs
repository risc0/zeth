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
use reth_evm::execute::{BatchExecutor, ExecutionOutcome, ProviderError};
use reth_evm_ethereum::execute::EthBatchExecutor;
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::Block;
use reth_revm::db::BundleState;
use std::fmt::Display;

pub trait PostExecutionValidationStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output<'b>;

    fn post_execution_validation(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>>;
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
