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

#![feature(path_file_prefix)]

use std::path::PathBuf;

use assert_cmd::Command;
use rstest::rstest;

#[rstest]
fn block_cli(#[files("testdata/*.json.gz")] mut path: PathBuf) {
    let block_no = String::from(path.file_prefix().unwrap().to_str().unwrap());

    path.pop();

    let mut cmd = Command::cargo_bin("zeth").unwrap();
    let assert = cmd
        .args(&[
            "--cache-path",
            path.to_str().unwrap(),
            "--block-no",
            &block_no,
        ])
        .assert();
    assert.success();
}
