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

use std::{collections::BTreeSet, path::PathBuf};

use anyhow::{anyhow, Result};
use ethers_core::types::{
    Block, Bytes, EIP1186ProofResponse, Transaction, TransactionReceipt, H160, H256, U256,
};
use serde::{Deserialize, Serialize};

pub mod cached_rpc_provider;
pub mod file_provider;
pub mod rpc_provider;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct AccountQuery {
    pub block_no: u64,
    pub address: H160,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct BlockQuery {
    pub block_no: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct ProofQuery {
    pub block_no: u64,
    pub address: H160,
    pub indices: BTreeSet<H256>,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct StorageQuery {
    pub block_no: u64,
    pub address: H160,
    pub index: H256,
}

pub trait Provider: Send {
    fn save(&self) -> Result<()>;

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>>;
    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>>;
    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>>;
    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse>;
    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256>;
    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256>;
    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes>;
    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256>;
}

pub trait MutProvider: Provider {
    fn insert_full_block(&mut self, query: BlockQuery, val: Block<Transaction>);
    fn insert_partial_block(&mut self, query: BlockQuery, val: Block<H256>);
    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<TransactionReceipt>);
    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186ProofResponse);
    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256);
    fn insert_balance(&mut self, query: AccountQuery, val: U256);
    fn insert_code(&mut self, query: AccountQuery, val: Bytes);
    fn insert_storage(&mut self, query: StorageQuery, val: H256);
}

pub fn new_file_provider(file_path: PathBuf) -> Result<Box<dyn Provider>> {
    let provider = file_provider::FileProvider::new(file_path)?;

    Ok(Box::new(provider))
}

pub fn new_rpc_provider(rpc_url: String) -> Result<Box<dyn Provider>> {
    let provider = rpc_provider::RpcProvider::new(rpc_url)?;

    Ok(Box::new(provider))
}

pub fn new_cached_rpc_provider(cache_path: PathBuf, rpc_url: String) -> Result<Box<dyn Provider>> {
    let provider = cached_rpc_provider::CachedRpcProvider::new(cache_path, rpc_url)?;

    Ok(Box::new(provider))
}

pub fn new_provider(
    cache_path: Option<PathBuf>,
    rpc_url: Option<String>,
) -> Result<Box<dyn Provider>> {
    match (cache_path, rpc_url) {
        (Some(cache_path), Some(rpc_url)) => new_cached_rpc_provider(cache_path, rpc_url),
        (Some(cache_path), None) => new_file_provider(cache_path),
        (None, Some(rpc_url)) => new_rpc_provider(rpc_url),
        (None, None) => Err(anyhow!("No cache_path or rpc_url given")),
    }
}
