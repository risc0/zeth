// Copyright 2025 RISC Zero, Inc.
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

#![deny(unsafe_code)]

use risc0_zkvm::guest::env;
use zeth_chainspec::ChainSpec;
use zeth_core::{EthEvmConfig, StatelessInput, validate_block};

pub fn entry(evm_config: EthEvmConfig<ChainSpec>) {
    let chain_spec = evm_config.chain_spec();
    env::log(&format!("EVM config: {chain_spec}"));

    env::log("cycle-tracker-report-start: read_input");
    let input: StatelessInput = env::read();
    env::log("cycle-tracker-report-end: read_input");

    env::log("cycle-tracker-report-start: validation");
    let block_hash = validate_block(input.block, input.witness, evm_config).unwrap();
    env::log("cycle-tracker-report-end: validation");

    env::commit_slice(block_hash.as_slice());
}
