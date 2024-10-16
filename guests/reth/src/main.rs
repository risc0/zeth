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
use reth_consensus::Consensus;
use reth_ethereum_consensus::EthBeaconConsensus;
// use c_kzg::KzgSettings;
use reth_evm::execute::{BatchExecutor, BlockExecutionInput, BlockExecutorProvider};
use reth_evm_ethereum::execute::EthExecutorProvider;
use reth_primitives::{Block, BlockWithSenders};
use reth_revm::InMemoryDB;
use risc0_zkvm::guest::env;

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
    let db = InMemoryDB::default();
    let mut executor = EthExecutorProvider::ethereum(MAINNET.clone()).batch_executor(db);
    let consensus = EthBeaconConsensus::new(MAINNET.clone());

    let block: Block = env::read();
    let total_difficulty = env::read();

    consensus
        .validate_header_with_total_difficulty(&block.header, total_difficulty)
        .expect("Failed to validate header with total difficulty");

    let sealed_block = block.seal_slow();

    consensus
        .validate_header(&sealed_block.header)
        .expect("Failed to validate header");
    // consensus.validate_header_against_parent(&sealed_block.header, todo!())
    //     .expect("Failed to validate header against parent");
    consensus
        .validate_block_pre_execution(&sealed_block)
        .expect("Failed to validate block");

    let block_hash = sealed_block.hash();

    let block_with_senders = BlockWithSenders {
        block: sealed_block.unseal(),
        senders: vec![], // todo: recover signers with non-det hints
    };
    let input = BlockExecutionInput {
        block: &block_with_senders,
        // todo: read in total chain difficulty
        total_difficulty: Default::default(),
    };
    executor
        .execute_and_verify_one(input)
        .expect("Execution failed");

    // consensus.validate_block_post_execution() is done as part of executor.execute_and_verify_one

    let outcome = executor.finalize();

    let _post_state = outcome.hash_state_slow();
    // todo: update state trie

    // todo: commit total chain difficulty
    env::commit_slice(&block_hash.0)
}
