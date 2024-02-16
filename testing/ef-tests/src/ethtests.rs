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

use std::{fs::File, io::BufReader, path::PathBuf};

use revm::primitives::SpecId;
use serde_json::Value;
use zeth_lib::consts::{ChainSpec, ETH_MAINNET_CHAIN_SPEC, ETH_MAINNET_EIP1559_CONSTANTS};
use zeth_primitives::block::Header;

use crate::TestJson;

pub struct EthTestCase {
    pub name: String,
    pub json: TestJson,
    pub genesis: Header,
    pub chain_spec: ChainSpec,
}

pub fn read_eth_test(path: PathBuf) -> Vec<EthTestCase> {
    println!("Using file: {}", path.display());
    let f = File::open(path).unwrap();
    let mut root: Value = serde_json::from_reader(BufReader::new(f)).unwrap();

    root.as_object_mut()
        .unwrap()
        .into_iter()
        .filter_map(|(name, test)| {
            let json: TestJson = serde_json::from_value(test.take()).unwrap();

            let spec: SpecId = json.network.replace("Paris", "Merge").as_str().into();
            if let Err(err) = ETH_MAINNET_CHAIN_SPEC.validate_spec_id(spec) {
                println!("skipping '{}': {}", name, err);
                return None;
            }
            let chain_spec = ChainSpec::new_single(1, spec, ETH_MAINNET_EIP1559_CONSTANTS);

            let genesis: Header = json.genesis.clone().into();
            assert_eq!(genesis.hash(), json.genesis.hash);

            Some(EthTestCase {
                name: name.clone(),
                json,
                genesis,
                chain_spec,
            })
        })
        .collect()
}
