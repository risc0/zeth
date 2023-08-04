// Copyright 2023 RISC Zero, Inc.
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

use risc0_zkvm::guest::env;
use zeth_lib::{block_builder::BlockBuilder, validation::Input};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let input: Input = env::read();

    let result = BlockBuilder::from(input)
        .initialize_evm_storage()
        .expect("Failed to create in-memory evm storage")
        .initialize_header()
        .expect("Failed to create the initial block header fields")
        .execute_transactions()
        .expect("Failed to execute transactions")
        .build(None)
        .expect("Failed to build the resulting block");

    env::commit(&result.hash());

    // Leak memory, save cycles
    core::mem::forget(result);
}
