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

use std::{path::PathBuf, str::FromStr};

use assert_cmd::Command;
use risc0_zkvm::{ExecutorEnv, ExecutorImpl, FileSegmentRef};
use rstest::rstest;
use tempfile::tempdir;
use zeth_guests::ETH_BLOCK_ELF;
use zeth_lib::{
    block_builder::EthereumStrategyBundle, consts::ETH_MAINNET_CHAIN_SPEC, input::Input,
};
use zeth_primitives::{transactions::ethereum::EthereumTxEssence, trie::MptNodeData};

#[rstest]
fn block_cli_ethereum(#[files("testdata/ethereum/*.json.gz")] path: PathBuf) {
    let block_no = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .args(["--cache=testdata", &format!("--block-no={}", block_no)])
        .assert()
        .success();
}

#[rstest]
fn empty_blocks(#[files("testdata/ethereum/*.json.gz")] path: PathBuf) {
    let block_no = u64::from_str(file_prefix(&path)).unwrap();
    // Set block cache directory
    let rpc_cache = Some(format!("testdata/ethereum/{}.json.gz", block_no));
    // Fetch all of the initial data
    let init = zeth_lib::host::get_initial_data::<EthereumStrategyBundle>(
        ETH_MAINNET_CHAIN_SPEC.clone(),
        rpc_cache,
        None,
        block_no,
    )
    .expect("Could not init");
    // Create input object
    let mut input: Input<EthereumTxEssence> = init.clone().into();
    // Take out transaction and withdrawal execution data
    input.transactions = Default::default();
    input.withdrawals = Default::default();
    input.contracts = Default::default();
    input.parent_state_trie = MptNodeData::Digest(input.parent_state_trie.hash()).into();
    input.parent_storage = Default::default();
    input.ancestor_headers = Default::default();
    // Prepare executor
    let env = ExecutorEnv::builder()
        .session_limit(None)
        .segment_limit_po2(20)
        .write(&input)
        .unwrap()
        .build()
        .unwrap();
    let mut exec = ExecutorImpl::from_elf(env, ETH_BLOCK_ELF).unwrap();
    // Run Executor
    let segment_dir = tempdir().unwrap();
    let session = exec
        .run_with_callback(|segment| {
            Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
        })
        .unwrap();
    // Output segment count
    println!("Generated {} segments", session.segments.len());
}

fn file_prefix(path: &PathBuf) -> &str {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    file_name.split('.').next().unwrap()
}
