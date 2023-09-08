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
use zeth_lib::optimism::DerivationInput;

risc0_zkvm::guest::entry!(main);

pub fn main() {
    // Read the input L1 and L2 data
    let input: DerivationInput = env::read();
    env::commit(&input.op_head.block_header.hash());
    // Process the optimism block derivation input
    let output = input.process().expect("Failed to process derivation input");
    // Output the resulting block's hash to the journal
    env::commit(&output.current_l1_block_hash);
    env::commit(&output.safe_head.hash);
}
