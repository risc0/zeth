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

use alloy::network::Ethereum;
use rstest::rstest;
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use zeth_preflight::{client::PreflightClient, BlockBuilder, Witness};
use zeth_preflight_ethereum::{RethBlockBuilder, RethPreflightDriver};
use zeth_testeth::{read_eth_execution_tests, TestCoreDriver, TestProvider};

#[rstest]
fn evm(
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*.json")]
    #[exclude("RevertPrecompiledTouch_storage.json|RevertPrecompiledTouch.json")] // precompiles having storage is not possible
    #[exclude("RevertInCreateInInit_Paris.json|RevertInCreateInInit.json|dynamicAccountOverwriteEmpty.json|dynamicAccountOverwriteEmpty_Paris.json|RevertInCreateInInitCreate2Paris.json|create2collisionStorage.json|RevertInCreateInInitCreate2.json|create2collisionStorageParis.json|InitCollision.json|InitCollisionParis.json")] // Test with some storage check
    #[exclude("stTimeConsuming")] // exclude only the time-consuming tests
    path: PathBuf,
) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    for mut json in read_eth_execution_tests(path) {
        // only one block supported for now
        if json.blocks.len() > 1 {
            println!("skipping (blockchain)");
            break;
        };
        let block = json.blocks.pop().unwrap();

        // skip failing tests for now
        if let Some(message) = block.expect_exception {
            println!("skipping ({})", message);
            break;
        }

        let expected_header = block.block_header.as_ref().unwrap();
        let expected_hash = expected_header.hash;

        // using the empty/default state for the input prepares all accounts for deletion
        // this leads to larger input, but can never fail
        let post_state = json.post_state.clone().unwrap_or_default();

        let provider = TestProvider::new(json.genesis_block_header, block, json.pre, post_state);
        let provider = Rc::new(RefCell::new(provider));

        let preflight_data =
            <<RethBlockBuilder as BlockBuilder<_, _, TestCoreDriver, _>>::PreflightClient as PreflightClient<
                Ethereum,
                TestCoreDriver,
                RethPreflightDriver,
            >>::preflight_with_provider(provider, 1, 1)
                .unwrap();
        let build_result = Witness::driver_from::<TestCoreDriver>(&preflight_data);

        // the header should match
        assert_eq!(build_result.validated_tip_hash, expected_hash);
    }
}
