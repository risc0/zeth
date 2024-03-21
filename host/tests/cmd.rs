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

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use rstest::rstest;

fn file_prefix(path: &Path) -> &str {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    file_name.split('.').next().unwrap()
}

#[rstest]
fn build_ethereum(#[files("testdata/ethereum/*.json.gz")] path: PathBuf) {
    let block_number = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .env("RUST_LOG", "info")
        .args([
            "build",
            "--network=ethereum",
            "--cache=testdata",
            &format!("--block-number={}", block_number),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(" WARN ").not());
}

#[rstest]
fn build_optimism(#[files("testdata/optimism/*.json.gz")] path: PathBuf) {
    let block_number = file_prefix(&path);

    Command::cargo_bin("zeth")
        .unwrap()
        .env("RUST_LOG", "info")
        .args([
            "build",
            "--network=optimism",
            "--cache=testdata",
            &format!("--block-number={}", block_number),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(" WARN ").not());
}

// #[rstest]
// #[case(109279674, 6)]
// fn build_optimism_derived(#[case] block_number: u64, #[case] block_count: u64) {
// Command::cargo_bin("zeth")
// .unwrap()
// .env("RUST_LOG", "info")
// .args([
// "build",
// "--network=optimism-derived",
// "--cache=testdata/derivation",
// &format!("--block-number={}", block_number),
// &format!("--block-count={}", block_count),
// ])
// .assert()
// .success()
// .stderr(predicate::str::contains(" WARN ").not());
//
// test composition
// Command::cargo_bin("zeth")
// .unwrap()
// .env("RUST_LOG", "info")
// .args([
// "build",
// "--network=optimism-derived",
// "--cache=testdata/derivation",
// &format!("--block-number={}", block_number),
// &format!("--block-count={}", block_count),
// "--composition=1",
// ])
// .assert()
// .success()
// .stderr(predicate::str::contains(" WARN ").not());
// }
