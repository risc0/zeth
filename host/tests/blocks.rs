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

use std::path::PathBuf;

use assert_cmd::Command;
use rstest::rstest;

#[rstest]
fn block_cli_ethereum(#[files("testdata/ethereum/*.json.gz")] path: PathBuf) {
    let block_no = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .args([
            "--network=ethereum",
            "--cache=testdata",
            &format!("--block-no={}", block_no),
        ])
        .assert()
        .success();
}

#[rstest]
fn block_cli_optimism(#[files("testdata/optimism/*.json.gz")] path: PathBuf) {
    let block_no = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .args([
            "--network=optimism",
            "--cache=testdata",
            &format!("--block-no={}", block_no),
        ])
        .assert()
        .success();
}

fn file_prefix(path: &PathBuf) -> &str {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    file_name.split('.').next().unwrap()
}
