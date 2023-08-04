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

use std::{fs::File, io::BufReader, path::PathBuf};

use common::ethers::TestProvider;
use hashbrown::HashMap;
use revm::primitives::SpecId;
use rstest::rstest;
use serde_json::Value;
use zeth_lib::{
    block_builder::BlockBuilder,
    consts::ChainSpec,
    host::{provider_db::ProviderDb, Init},
    mem_db::MemDb,
    validation::Input,
};
use zeth_primitives::{block::Header, transaction::Transaction, withdrawal::Withdrawal};

use crate::common::*;

mod common;

// TODO: investigate those stack overflows
static IGNORE_SET: phf::Set<&'static str> = phf::phf_set! {
    "baseFeeDiffPlaces_d34g0v0_Shanghai", "gasPriceDiffPlaces_d34g0v0_Shanghai", "LoopCallsDepthThenRevert2_d0g0v0_Shanghai",
    "LoopCallsDepthThenRevert3_d0g0v0_Shanghai", "diffPlaces_d34g0v0_Shanghai", "static_Call1024BalanceTooLow2_d1g0v0_Shanghai",
    "static_Call1024BalanceTooLow_d1g0v0_Shanghai", "static_Call1024PreCalls3_d1g0v0_Shanghai",
    "static_Call1024PreCalls_d1g0v0_Shanghai", "static_Call1024PreCalls2_d0g0v0_Shanghai",
    "static_Call1024PreCalls2_d1g0v0_Shanghai", "static_CallRecursiveBomb0_OOG_atMaxCallDepth_d0g0v0_Shanghai",
    "static_CallRecursiveBombPreCall2_d0g0v0_Shanghai", "static_CallRecursiveBombPreCall_d0g0v0_Shanghai",
    "static_LoopCallsDepthThenRevert3_d0g0v0_Shanghai", "static_LoopCallsDepthThenRevert2_d0g0v0_Shanghai",
    "CallRecursiveBomb0_OOG_atMaxCallDepth_d0g0v0_Shanghai", "Call1024BalanceTooLow_d0g0v0_Shanghai",
    "Call1024PreCalls_d0g1v0_Shanghai", "Call1024PreCalls_d0g2v0_Shanghai", "CallRecursiveBombPreCall_d0g0v0_Shanghai",
    "Delegatecall1024_d0g0v0_Shanghai", "Create2OnDepth1024_d0g0v0_Shanghai", "Create2OnDepth1023_d0g0v0_Shanghai",
    "Create2Recursive_d0g0v0_Shanghai", "Create2Recursive_d0g2v0_Shanghai", "Call1024PreCalls_d0g0v0_Shanghai",
    "Callcode1024BalanceTooLow_d0g0v0_Shanghai", "invalidDiffPlaces_d34g0v0_Shanghai", "opc0EDiffPlaces_d34g0v0_Shanghai",
};

#[rstest]
fn evm(
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*.json")]
    #[exclude("stBadOpcode|refundResetFrontier")]
    path: PathBuf,
) {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    println!("Using file: {}", path.display());
    let f = File::open(path).unwrap();
    let mut root: Value = serde_json::from_reader(BufReader::new(f)).unwrap();

    for (name, test) in root.as_object_mut().unwrap() {
        println!("test '{}'", name);
        if IGNORE_SET.contains(name) {
            println!("ignoring");
            continue;
        }
        let json: TestJson = serde_json::from_value(test.take()).unwrap();

        // only run Shanghai tests
        let spec: SpecId = json.network.as_str().into();
        if spec != SpecId::SHANGHAI {
            println!("skipping ({:?})", spec);
            continue;
        }
        let config = ChainSpec::new_single(1, spec);

        let genesis: Header = json.genesis.clone().into();
        assert_eq!(genesis.hash(), json.genesis.hash);

        // log the pre-state
        dbg!(&json.pre);

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

            // construct the block
            let builder = new_builder(
                config.clone(),
                state,
                parent_header.clone(),
                expected_header.clone(),
                block.transactions,
                block.withdrawals.unwrap_or_default(),
            )
            .initialize_evm_storage()
            .unwrap()
            .initialize_header()
            .unwrap()
            .execute_transactions()
            .unwrap();
            let result_header = builder.clone().build(None).unwrap();
            // the headers should match
            assert_eq!(result_header.state_root, expected_header.state_root);
            assert_eq!(result_header, expected_header);

            state = builder.to_db().into();
            ancestor_headers.push(parent_header);
            parent_header = block_header.into();
        }
        // log the post-state
        dbg!(state, &json.post);
    }
}

fn new_builder(
    config: ChainSpec,
    state: TestState,
    parent_header: Header,
    header: Header,
    transactions: Vec<TestTransaction>,
    withdrawals: Vec<Withdrawal>,
) -> BlockBuilder<MemDb> {
    // create the provider DB
    let mut provider_db = ProviderDb::new(
        Box::new(TestProvider {
            state,
            header: parent_header.clone(),
        }),
        parent_header.number,
    );

    let transactions: Vec<Transaction> = transactions.into_iter().map(Transaction::from).collect();
    let input = Input {
        beneficiary: header.beneficiary,
        gas_limit: header.gas_limit,
        timestamp: header.timestamp,
        extra_data: header.extra_data.clone(),
        mix_hash: header.mix_hash,
        transactions: transactions.clone(),
        withdrawals: withdrawals.clone(),
        chain_spec: config.clone(),
        parent_header: parent_header.clone(),
        ..Default::default()
    };

    // create and run the block builder once to create the initial DB
    let block_builder = BlockBuilder::new(Some(provider_db), input)
        .initialize_header()
        .unwrap()
        .execute_transactions()
        .unwrap();
    provider_db = block_builder.to_db();

    let init_proofs = provider_db.get_initial_proofs().unwrap();
    let fini_proofs = HashMap::new();
    let ancestor_headers = provider_db.get_ancestor_headers().unwrap();

    let input: Input = Init {
        db: provider_db.get_initial_db().clone(),
        init_block: parent_header,
        init_proofs,
        fini_block: header,
        fini_transactions: transactions,
        fini_withdrawals: withdrawals,
        fini_proofs,
        ancestor_headers,
        chain_spec: config,
    }
    .into();

    input.into()
}
