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

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use rstest::rstest;

fn file_prefix(path: &Path) -> &str {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    file_name.split('.').next().unwrap()
}

#[rstest]
fn zeth_ethereum(#[files("testdata/ethereum/*.json.gz")] path: PathBuf) {
    let block_no = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .args([
            "build",
            "--network=ethereum",
            "--cache=testdata",
            &format!("--block-number={}", block_no),
        ])
        .assert()
        .success();
}

#[rstest]
fn zeth_optimism(#[files("testdata/optimism/*.json.gz")] path: PathBuf) {
    let block_no = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .args([
            "build",
            "--network=optimism",
            "--cache=testdata",
            &format!("--block-number={}", block_no),
        ])
        .assert()
        .success();
}

#[rstest]
#[case(109279674, 6)]
fn derive_optimism(#[case] op_block_no: u64, #[case] op_blocks: u64) {
    Command::cargo_bin("zeth")
        .unwrap()
        .args([
            "build",
            "--network=optimism-derived",
            "--cache=testdata/derivation",
            &format!("--block-number={}", op_block_no),
            &format!("--block-count={}", op_blocks),
        ])
        .assert()
        .success();
}
