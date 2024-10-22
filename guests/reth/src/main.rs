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
use zeth_core::stateless::client::{RethStatelessClient, StatelessClient};
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
    let stateless_client_block = env::read();

    let block_hash = RethStatelessClient::validate_block(
        MAINNET.clone(),
        stateless_client_block,
    )
    .expect("block validation failed");

    // todo: commit total chain difficulty
    env::commit_slice(&block_hash.0)
}
