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
use zeth_lib::{
    block_builder::BlockBuilder, execution::ethereum::EthTxExecStrategy,
    finalization::BuildFromMemDbStrategy, initialization::MemDbInitStrategy,
    preparation::EthHeaderPrepStrategy,
};
use zeth_primitives::block::Header;
use zeth_testeth::{
    ethtests::{read_eth_test, EthTestCase},
    *,
};

#[rstest]
fn evm(
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*.json")]
    #[exclude("stTimeConsuming")] // exclude only the time consuming tests
    path: PathBuf,
) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    for EthTestCase {
        json,
        genesis,
        chain_spec,
    } in read_eth_test(path)
    {
        let mut state = json.pre;
        let mut parent_header = genesis;
        let mut ancestor_headers = vec![];
        for block in json.blocks {
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
            let builder = BlockBuilder::new(&chain_spec, input)
                .initialize_database::<MemDbInitStrategy>()
                .unwrap()
                .prepare_header::<EthHeaderPrepStrategy>()
                .unwrap();
            // execute the transactions with a larger stack
            let builder = stacker::grow(BIG_STACK_SIZE, move || {
                builder.execute_transactions::<EthTxExecStrategy>().unwrap()
            });
            // update the state
            state = builder.db().unwrap().into();

            let result_header = builder.build::<BuildFromMemDbStrategy>().unwrap();
            // the headers should match
            assert_eq!(result_header.state_root, expected_header.state_root);
            assert_eq!(result_header, expected_header);

            // update the headers
            ancestor_headers.push(parent_header);
            parent_header = block_header.into();
        }
    }
}
