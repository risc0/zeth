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

#![no_main]

use std::sync::Arc;
use reth_chainspec::ChainSpecBuilder;
use reth_evm_ethereum::EthEvmConfig;
use reth_evm_ethereum::execute::EthBlockExecutor;
use reth_execution_types::BlockExecutionInput;
use reth_storage_errors::provider::ProviderError;
use reth_evm::execute::Executor;
use revm::db::EmptyDBTyped;
use revm::StateBuilder;

risc0_zkvm::guest::entry!(main);

fn main() {

    let chain_spec = Arc::new(ChainSpecBuilder::mainnet().build());
    let evm_config = EthEvmConfig::default();
    let state = StateBuilder::new_with_database(EmptyDBTyped::<ProviderError>::default()).build();
    let mut executor = EthBlockExecutor::new(
        chain_spec,
        evm_config,
        state
    );

    // let input = BlockExecutionInput::new(
    //     BlockW
    // )




}
