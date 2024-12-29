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

use risc0_zkvm::guest::env;
use zeth_core::db::trie::TrieDB;
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::data::{ArchivedCommonData, ChainData, StatelessClientData};
use zeth_core_ethereum::{RethCoreDriver, RethStatelessClient};

#[no_mangle]
pub extern "C" fn __ctzsi2(x: u32) -> usize {
    x.trailing_zeros() as usize
}

fn main() {
    // todo: load up revm with hashbrown feat
    let common_data_bytes = env::read_frame();
    let chain_data_bytes = env::read_frame();
    env::log("Deserializing input data");

    let common_data_rkyv = rkyv::access::<ArchivedCommonData, rkyv::rancor::Error>(&common_data_bytes).expect("Rkyv failed to access CommonData instance");
    let chain_data_pot = pot::from_slice::<ChainData<
        <RethCoreDriver as CoreDriver>::Block, <RethCoreDriver as CoreDriver>::Header>>(&chain_data_bytes).expect("pot failed to deserialize data");
    env::log("Validating trie data");
    common_data_rkyv.state_trie.verify_reference().expect("Failed to validate state trie.");
    env::log("Constructing client data");
    let stateless_client_data = StatelessClientData::<'_, <RethCoreDriver as CoreDriver>::Block, <RethCoreDriver as CoreDriver>::Header>::from_rkyv(
        common_data_rkyv,
        chain_data_pot
    ).expect("StatelessClientData construction failed");

    // let stateless_client_data =
    //     <RethStatelessClient as StatelessClient<RethCoreDriver, TrieDB>>::data_from_parts(
    //         &common_data_bytes,
    //         &chain_data_bytes,
    //     )
    //     .expect("Failed to load client data from stdin");

    let validation_depth = stateless_client_data.blocks.len() as u64;
    assert!(
        stateless_client_data.chain.is_ethereum(),
        "This program only supports Ethereum chains"
    );
    let chain_id = stateless_client_data.chain as u64;
    // Build the block
    env::log("Validating blocks");
    let engine = <RethStatelessClient as StatelessClient<RethCoreDriver, TrieDB>>::validate(
        stateless_client_data,
    )
    .expect("block validation failed");
    // Build the journal (todo: make this a strategy)
    let block_hash = engine.data.parent_header.hash_slow();
    let total_difficulty = engine.data.total_difficulty;
    let journal = [
        chain_id.to_be_bytes().as_slice(),
        block_hash.0.as_slice(),
        total_difficulty.to_be_bytes::<32>().as_slice(),
        validation_depth.to_be_bytes().as_slice(),
    ]
    .concat();
    env::commit_slice(&journal.as_slice())
}
