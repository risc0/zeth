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

use rstest::rstest;
use zeth_lib::{
    builder::{BlockBuilderStrategy, EthereumStrategy},
    output::BlockBuildOutput,
};
use zeth_primitives::{block::Header, trie::StateAccount};
use zeth_testeth::{
    create_input, ethers,
    ethtests::{read_eth_test, EthTestCase},
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

        // using the empty/default state for the input prepares all accounts for deletion
        // this leads to larger input, but can never fail
        let post_state = json.post.clone().unwrap_or_default();

        let input = create_input(
            &chain_spec,
            genesis,
            json.pre,
            expected_header.clone(),
            block.transactions,
            block.withdrawals.unwrap_or_default(),
            post_state,
        );
        let input_state_input_hash = input.state_input.hash();

        let output = EthereumStrategy::build_from(&chain_spec, input).unwrap();

        let BlockBuildOutput::SUCCESS {
            hash: new_block_hash,
            head: new_block_head,
            state: new_block_state,
            state_input_hash,
        } = output
        else {
            panic!("Invalid block")
        };

        if let Some(post) = json.post {
            let (exp_state, _) = ethers::build_tries(&post);

            println!("diffing state trie:");
            for diff in diff::slice(
                &new_block_state.debug_rlp::<StateAccount>(),
                &exp_state.debug_rlp::<StateAccount>(),
            ) {
                match diff {
                    diff::Result::Left(l) => println!("✗{}", l),
                    diff::Result::Right(r) => println!("✓{}", r),
                    diff::Result::Both(l, _) => println!(" {}", l),
                }
            }
            assert_eq!(new_block_state.hash(), exp_state.hash());
        }

        // the headers should match
        assert_eq!(new_block_head, expected_header);
        assert_eq!(new_block_hash, expected_header.hash());
        assert_eq!(input_state_input_hash, state_input_hash);
    }
}
