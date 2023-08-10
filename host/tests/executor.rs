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

use risc0_zkvm::{serde::from_slice, ExecutorEnv, FileSegmentRef, LocalExecutor};
use rstest::rstest;
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};
use tempfile::tempdir;
use zeth_guests::ETH_BLOCK_ELF;
use zeth_primitives::BlockHash;

const SEGMENT_LIMIT_PO2: usize = 21;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct Test {
    #[serde_as(as = "Base64")]
    input: Vec<u8>,
    hash: BlockHash,
}

#[rstest]
fn executor(#[files("testdata/input/*.json")] path: PathBuf) {
    println!("Using file: {}", path.display());
    let f = File::open(path).unwrap();
    let test: Test = serde_json::from_reader(BufReader::new(f)).unwrap();

    let env = ExecutorEnv::builder()
        .session_limit(None)
        .segment_limit_po2(SEGMENT_LIMIT_PO2)
        .add_input(&test.input)
        .build()
        .unwrap();
    let mut exec = LocalExecutor::from_elf(env, ETH_BLOCK_ELF).unwrap();

    let segment_dir = tempdir().unwrap();
    let session = exec
        .run_with_callback(|segment| {
            Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
        })
        .unwrap();
    println!("Generated {} segments", session.segments.len());

    let found_hash: BlockHash = from_slice(&session.journal).unwrap();
    println!("Block hash (from executor): {}", found_hash);
    assert_eq!(found_hash, test.hash);
}
