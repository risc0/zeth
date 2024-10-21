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

use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::rpc::types::{Block, EIP1186AccountProofResponse, Transaction, TransactionReceipt};
use anyhow::anyhow;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

pub mod cache_provider;
pub mod db;
pub mod file_provider;
pub mod rpc_provider;

pub fn new_file_provider(file_path: PathBuf) -> anyhow::Result<Arc<RefCell<dyn Provider>>> {
    Ok(Arc::new(RefCell::new(file_provider::FileProvider::new(
        file_path,
    )?)))
}

pub fn new_rpc_provider(rpc_url: String) -> anyhow::Result<Arc<RefCell<dyn Provider>>> {
    Ok(Arc::new(RefCell::new(rpc_provider::RpcProvider::new(
        rpc_url,
    )?)))
}

pub fn new_cached_rpc_provider(
    cache_path: PathBuf,
    rpc_url: String,
) -> anyhow::Result<Arc<RefCell<dyn Provider>>> {
    Ok(Arc::new(RefCell::new(
        cache_provider::CachedRpcProvider::new(cache_path, rpc_url)?,
    )))
}

pub fn new_provider(
    cache_path: Option<PathBuf>,
    rpc_url: Option<String>,
) -> anyhow::Result<Arc<RefCell<dyn Provider>>> {
    match (cache_path, rpc_url) {
        (Some(cache_path), Some(rpc_url)) => new_cached_rpc_provider(cache_path, rpc_url),
        (Some(cache_path), None) => new_file_provider(cache_path),
        (None, Some(rpc_url)) => new_rpc_provider(rpc_url),
        (None, None) => Err(anyhow!("No cache_path or rpc_url given")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct AccountQuery {
    pub block_no: u64,
    pub address: Address,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct BlockQuery {
    pub block_no: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct UncleQuery {
    pub uncle_hash: B256,
    pub index_number: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct ProofQuery {
    pub block_no: u64,
    pub address: Address,
    pub indices: BTreeSet<B256>,
}

#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct StorageQuery {
    pub block_no: u64,
    pub address: Address,
    pub index: U256,
}

pub trait Provider: Send {
    fn save(&self) -> anyhow::Result<()>;

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<Block<Transaction>>;
    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<Block<Transaction>>;
    fn get_block_receipts(&mut self, query: &BlockQuery)
        -> anyhow::Result<Vec<TransactionReceipt>>;
    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse>;
    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256>;
    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256>;
    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes>;
    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256>;
}

pub trait MutProvider: Provider {
    fn insert_full_block(&mut self, query: BlockQuery, val: Block<Transaction>);
    fn insert_uncle_block(&mut self, query: UncleQuery, val: Block<Transaction>);
    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<TransactionReceipt>);
    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186AccountProofResponse);
    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256);
    fn insert_balance(&mut self, query: AccountQuery, val: U256);
    fn insert_code(&mut self, query: AccountQuery, val: Bytes);
    fn insert_storage(&mut self, query: StorageQuery, val: U256);
}

pub fn get_proofs(
    provider: &mut dyn Provider,
    block_no: u64,
    storage_keys: HashMap<Address, Vec<U256>>,
) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
    let mut out = HashMap::new();

    for (address, indices) in storage_keys {
        let proof = {
            let address: Address = address.into_array().into();
            let indices: BTreeSet<B256> = indices
                .into_iter()
                .map(|x| x.to_be_bytes().into())
                .collect();
            provider.get_proof(&ProofQuery {
                block_no,
                address,
                indices,
            })?
        };
        out.insert(address, proof);
    }

    Ok(out)
}
