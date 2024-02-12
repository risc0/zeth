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
use zeth_lib::optimism::{
    batcher_db::MemDb, config::OPTIMISM_CHAIN_SPEC, DeriveInput, DeriveMachine,
};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let derive_input: DeriveInput<MemDb> = env::read();
    let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input, None)
        .expect("Could not create derive machine");
    let output = derive_machine
        .derive(None)
        .expect("Failed to process derivation input");
    env::commit(&output);
}
