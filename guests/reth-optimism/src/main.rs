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

use reth_optimism_chainspec::OP_MAINNET;
// use c_kzg::KzgSettings;
use risc0_zkvm::guest::env;
use risc0_zkvm::guest::env::stdin;
use zeth_core::stateless::client::StatelessClient;
use zeth_core_optimism::OpRethStatelessClient;
// todo: use this instead of the alloy KzgEnv to save cycles
// lazy_static::lazy_static! {
//     /// KZG Ceremony data
//     pub static ref KZG: (Vec<u8>, KzgSettings) = {
//         let mut data = Vec::from(include_bytes!("../kzg_settings_raw.bin"));
//         let settings = KzgSettings::from_u8_slice(&mut data);
//         (data, settings)
//     };
// }

#[no_mangle]
pub extern "C" fn __ctzsi2(x: u32) -> usize {
    x.trailing_zeros() as usize
}

fn main() {
    // todo: load up revm with hashbrown feat
    let stateless_client_data = OpRethStatelessClient::deserialize_data(stdin())
        .expect("Failed to load client data from stdin");
    let validation_depth = stateless_client_data.blocks.len() as u64;
    // Build the block
    env::log("Validating blocks");
    let engine = OpRethStatelessClient::validate(OP_MAINNET.clone(), stateless_client_data)
        .expect("block validation failed");
    // Build the journal (todo: make this a strategy)
    let block_hash = engine.data.parent_header.hash_slow();
    let total_difficulty = engine.data.total_difficulty;
    let journal = [
        block_hash.0.as_slice(),
        total_difficulty.to_be_bytes::<32>().as_slice(),
        validation_depth.to_be_bytes().as_slice(),
    ]
    .concat();
    env::commit_slice(&journal.as_slice())
}
