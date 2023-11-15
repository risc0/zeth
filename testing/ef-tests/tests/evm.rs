// Copyright 2023 RISC Zero, Inc.
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

use rstest::rstest;
use zeth_lib::builder::{BlockBuilderStrategy, EthereumStrategy};
use zeth_primitives::block::Header;
use zeth_testeth::{
    ethtests::{read_eth_test, EthTestCase},
    *,
};

#[rstest]
fn evm(
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*.json")]
    #[exclude("RevertPrecompiledTouch_storage.json|RevertPrecompiledTouch.json")] // precompiles having storage is not possible
    #[exclude("stTimeConsuming")] // exclude only the time consuming tests
    path: PathBuf,
) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    for EthTestCase {
        mut json,
        genesis,
        chain_spec,
    } in read_eth_test(path)
    {
        let state = json.pre;
        let parent_header = genesis;
        // only one block supported for now
        assert_eq!(json.blocks.len(), 1);
        let block = json.blocks.pop().unwrap();

        // skip failing tests for now
        if let Some(message) = block.expect_exception {
            println!("skipping ({})", message);
            break;
        }

        let block_header = block.block_header.unwrap();
        let expected_header: Header = block_header.clone().into();
        assert_eq!(&expected_header.hash(), &block_header.hash);

        let input = create_input(
            &chain_spec,
            state,
            parent_header.clone(),
            expected_header.clone(),
            block.transactions,
            block.withdrawals.unwrap_or_default(),
        );

        let (header, _) = EthereumStrategy::build_from(&chain_spec, input).unwrap();

        // the headers should match
        assert_eq!(header, expected_header);
    }
}
