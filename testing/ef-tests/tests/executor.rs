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

#![cfg(feature = "ef-tests")]

use std::path::PathBuf;

use risc0_zkvm::{ExecutorEnv, ExecutorImpl, FileSegmentRef};
use rstest::rstest;
use tempfile::tempdir;
use zeth_lib::output::BlockBuildOutput;
use zeth_primitives::block::Header;
use zeth_testeth::{
    create_input,
    ethtests::{read_eth_test, EthTestCase},
    guests::TEST_GUEST_ELF,
};

const SEGMENT_LIMIT_PO2: u32 = 21;

#[rstest]
fn executor(
    // execute only the deep stack tests
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*Call1024BalanceTooLow.json")]
    path: PathBuf,
) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .filter_module("risc0_zkvm", log::LevelFilter::Info) // reduce noise
        .is_test(true)
        .try_init();

    for EthTestCase {
        name,
        json,
        genesis,
        chain_spec,
    } in read_eth_test(path)
    {
        // only one block
        assert_eq!(json.blocks.len(), 1usize);
        let block = json.blocks.first().unwrap().clone();

        // skip failing tests for now
        if let Some(message) = block.expect_exception {
            println!("skipping ({})", message);
            break;
        }

        println!("running: {}", name);

        let block_header = block.block_header.unwrap();
        let expected_header: Header = block_header.clone().into();
        assert_eq!(&expected_header.hash(), &block_header.hash);

        let input = create_input(
            &chain_spec,
            genesis,
            json.pre,
            expected_header.clone(),
            block.transactions,
            block.withdrawals,
            json.post.unwrap(),
        );

        let env = ExecutorEnv::builder()
            .session_limit(None)
            .segment_limit_po2(SEGMENT_LIMIT_PO2)
            .write(&chain_spec)
            .unwrap()
            .write(&input)
            .unwrap()
            .build()
            .unwrap();
        let mut exec = ExecutorImpl::from_elf(env, TEST_GUEST_ELF).unwrap();

        let segment_dir = tempdir().unwrap();
        let session = exec
            .run_with_callback(|segment| {
                Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
            })
            .unwrap();
        println!("Generated {} segments", session.segments.len());

        let build_output: BlockBuildOutput = session.journal.unwrap().decode().unwrap();
        let BlockBuildOutput::SUCCESS {
            hash: new_block_hash,
            ..
        } = build_output
        else {
            panic!("Block build failed!")
        };
        println!("Block hash (from executor): {}", new_block_hash);
        assert_eq!(new_block_hash, expected_header.hash());
    }
}
