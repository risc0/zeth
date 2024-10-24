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

use reth_chainspec::MAINNET;
// use c_kzg::KzgSettings;
use risc0_zkvm::guest::env;
use risc0_zkvm::guest::env::stdin;
use zeth_core::stateless::client::{RethStatelessClient, StatelessClient};
use zeth_core::SERDE_BRIEF_CFG;
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
    let stateless_client_data =
        serde_brief::from_reader_with_config(stdin(), SERDE_BRIEF_CFG).unwrap();
    env::log("Validating block");
    let (block_hash, total_difficulty, validation_depth) =
        RethStatelessClient::validate(MAINNET.clone(), stateless_client_data)
            .expect("block validation failed");

    let journal = [
        block_hash.0,
        total_difficulty.to_be_bytes::<32>(),
        validation_depth.to_be_bytes::<32>(),
    ].concat();
    env::commit_slice(&journal.as_slice())
}
