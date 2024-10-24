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

use alloy::primitives::B256;
use risc0_zkvm::{is_dev_mode, ProverOpts};
use zeth_core::keccak::keccak;

pub mod chain;
pub mod cli;
pub mod client;
pub mod executor;

pub fn proof_file_name(
    first_block_hash: B256,
    last_block_hash: B256,
    image_id: [u32; 8],
    prover_opts: &ProverOpts,
) -> String {
    let prover_opts = bincode::serialize(prover_opts).unwrap();
    let version = risc0_zkvm::get_version().unwrap();
    let suffix = if is_dev_mode() { "fake" } else { "zkp" };
    let data = [
        bytemuck::cast::<_, [u8; 32]>(image_id).as_slice(),
        first_block_hash.as_slice(),
        last_block_hash.as_slice(),
        prover_opts.as_slice(),
    ]
    .concat();
    let file_name = B256::from(keccak(data));
    format!("risc0-{}-{file_name}.{suffix}", version.to_string())
}
