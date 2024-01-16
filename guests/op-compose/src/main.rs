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

use risc0_zkvm::guest::env;
use zeth_lib::optimism::composition::ComposeInput;

risc0_zkvm::guest::entry!(main);

pub fn main() {
    // read composition input
    let compose_input: ComposeInput = env::read();
    // process input
    let compose_output = compose_input.process().expect("Failed to process composition.");
    // output statement about larger segment
    env::commit(&compose_output);
}
