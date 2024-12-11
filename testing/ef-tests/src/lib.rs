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

use alloy::{
    primitives::{Address, Bytes, B256, U256},
    rpc::types::{Header, Transaction, Withdrawal},
};
use serde::Deserialize;
use serde_with::{serde_as, TryFromInto};
use std::{collections::HashMap, default::Default, fs::File, io::BufReader, path::PathBuf};

mod driver;
mod provider;

pub use driver::TestCoreDriver;
pub use provider::TestProvider;

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Test {
    #[serde_as(as = "TryFromInto<TestHeader>")]
    pub genesis_block_header: Header,
    #[serde(rename = "genesisRLP")]
    pub genesis_rlp: Bytes,
    pub blocks: Vec<TestBlock>,
    pub network: String,
    pub pre: TestState,
    pub post_state: Option<TestState>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct TestHeader(serde_json::Map<String, serde_json::Value>);

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBlock {
    #[serde_as(as = "Option<TryFromInto<TestHeader>>")]
    pub block_header: Option<Header>,
    pub expect_exception: Option<String>,
    pub rlp: Bytes,
    #[serde(default)]
    #[serde_as(as = "Vec<TryFromInto<TestTransaction>>")]
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    #[serde_as(as = "Vec<TryFromInto<TestHeader>>")]
    pub uncle_headers: Vec<Header>,
    pub withdrawals: Option<Vec<Withdrawal>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct TestTransaction(serde_json::Map<String, serde_json::Value>);

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestState(HashMap<Address, TestAccount>);

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccount {
    pub balance: U256,
    #[serde(with = "alloy::serde::quantity")]
    pub nonce: u64,
    pub storage: HashMap<U256, U256>,
    pub code: Bytes,
}

impl TryFrom<TestHeader> for Header {
    type Error = serde_json::Error;

    fn try_from(h: TestHeader) -> Result<Self, Self::Error> {
        let mut map = h.0;

        // rename some fields
        map.remove("uncleHash")
            .map(|v| map.insert("sha3Uncles".to_string(), v));
        map.remove("coinbase")
            .map(|v| map.insert("miner".to_string(), v));
        map.remove("transactionsTrie")
            .map(|v| map.insert("transactionsRoot".to_string(), v));
        map.remove("receiptTrie")
            .map(|v| map.insert("receiptsRoot".to_string(), v));
        map.remove("bloom")
            .map(|v| map.insert("logsBloom".to_string(), v));

        serde_json::from_value(map.into())
    }
}

impl TryFrom<TestTransaction> for Transaction {
    type Error = serde_json::Error;

    fn try_from(tx: TestTransaction) -> Result<Self, Self::Error> {
        let mut map = tx.0;

        // rename some fields
        map.remove("data")
            .map(|v| map.insert("input".to_string(), v));
        map.remove("gasLimit")
            .map(|v| map.insert("gas".to_string(), v));

        // add defaults for missing fields
        map.entry("hash")
            .or_insert_with(|| serde_json::to_value(B256::default()).unwrap());
        map.entry("from")
            .or_insert_with(|| serde_json::to_value(Address::default()).unwrap());

        // it seems that for pre-EIP-155 txs, the chain ID is sometimes incorrectly set to 0
        if let serde_json::map::Entry::Occupied(entry) = map.entry("chainId") {
            if entry.get().as_str() == Some("0x00") {
                entry.remove();
            }
        }
        // recipient field should be missing instead of empty
        if let serde_json::map::Entry::Occupied(entry) = map.entry("to") {
            if entry.get().as_str() == Some("") {
                entry.remove();
            }
        }

        serde_json::from_value(map.into())
    }
}

pub fn read_eth_execution_tests(path: PathBuf) -> impl Iterator<Item = Test> {
    println!("Using file: {}", path.display());
    let f = File::open(path).unwrap();

    let tests: HashMap<String, Test> = serde_json::from_reader(BufReader::new(f)).unwrap();
    tests.into_iter().filter_map(|(name, test)| {
        println!("test '{}'", name);

        // skip tests with an unsupported network version
        if test.network.as_str() != "Cancun" {
            println!("skipping ({})", test.network);
            return None;
        }

        Some(test)
    })
}
