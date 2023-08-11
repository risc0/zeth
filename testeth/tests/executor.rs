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

use revm::primitives::SpecId;
use risc0_zkvm::{
    serde::{from_slice, to_vec},
    ExecutorEnv, FileSegmentRef, LocalExecutor,
};
use rstest::rstest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{base64::Base64, serde_as};
use tempfile::tempdir;
use zeth_lib::consts::ChainSpec;
use zeth_primitives::{block::Header, BlockHash};
use zeth_testeth::{guests::TEST_GUEST_ELF, new_builder, TestJson};

const SEGMENT_LIMIT_PO2: usize = 21;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
struct Test {
    #[serde_as(as = "Base64")]
    input: Vec<u8>,
    hash: BlockHash,
}

#[rstest]
fn executor(
    // execute only the deep stack tests
    #[files("testdata/BlockchainTests/GeneralStateTests/**/*Call1024BalanceTooLow.json")]
    path: PathBuf,
) {
    println!("Using file: {}", path.display());

    let f = File::open(path).unwrap();
    let mut root: Value = serde_json::from_reader(BufReader::new(f)).unwrap();

    for (name, test) in root.as_object_mut().unwrap() {
        println!("test '{}'", name);
        let json: TestJson = serde_json::from_value(test.take()).unwrap();

        let spec: SpecId = json.network.as_str().into();
        // skip tests with an unsupported network version
        if spec < SpecId::MERGE || spec > SpecId::SHANGHAI {
            println!("skipping ({})", json.network);
            continue;
        }
        let chain_spec = ChainSpec::new_single(1, spec, Default::default());

        let genesis: Header = json.genesis.clone().into();
        assert_eq!(genesis.hash(), json.genesis.hash);

        // log the pre-state
        dbg!(&json.pre);

        // only one block
        assert_eq!(json.blocks.len(), 1usize);
        let block = json.blocks.first().unwrap().clone();

        // skip failing tests for now
        if let Some(message) = block.expect_exception {
            println!("skipping ({})", message);
            break;
        }

        let block_header = block.block_header.unwrap();
        let expected_header: Header = block_header.clone().into();
        assert_eq!(&expected_header.hash(), &block_header.hash);

        let builder = new_builder(
            chain_spec.clone(),
            json.pre,
            genesis,
            expected_header.clone(),
            block.transactions,
            block.withdrawals.unwrap_or_default(),
        );

        let env = ExecutorEnv::builder()
            .session_limit(None)
            .segment_limit_po2(SEGMENT_LIMIT_PO2)
            .add_input(&to_vec(&chain_spec).unwrap())
            .add_input(&to_vec(&builder.input).unwrap())
            .build()
            .unwrap();
        let mut exec = LocalExecutor::from_elf(env, TEST_GUEST_ELF).unwrap();

        let segment_dir = tempdir().unwrap();
        let session = exec
            .run_with_callback(|segment| {
                Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
            })
            .unwrap();
        println!("Generated {} segments", session.segments.len());

        let found_hash: BlockHash = from_slice(&session.journal).unwrap();
        println!("Block hash (from executor): {}", found_hash);
        assert_eq!(found_hash, expected_header.hash());
    }
}
