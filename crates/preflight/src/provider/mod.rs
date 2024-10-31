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

use alloy::network::Network;
use alloy::primitives::map::HashMap;
use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::rpc::types::EIP1186AccountProofResponse;
use anyhow::anyhow;
use query::{AccountQuery, BlockQuery, ProofQuery, StorageQuery, UncleQuery};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

pub mod cache_provider;
pub mod db;
pub mod file_provider;
pub mod query;
pub mod rpc_provider;

pub fn new_file_provider<N: Network>(
    dir_path: PathBuf,
    block_no: u64,
) -> anyhow::Result<Rc<RefCell<dyn Provider<N>>>> {
    Ok(Rc::new(RefCell::new(file_provider::FileProvider::new(
        dir_path, block_no,
    )?)))
}

pub fn new_rpc_provider<N: Network>(
    rpc_url: String,
) -> anyhow::Result<Rc<RefCell<dyn Provider<N>>>> {
    Ok(Rc::new(RefCell::new(rpc_provider::RpcProvider::new(
        rpc_url,
    )?)))
}

pub fn new_cached_rpc_provider<N: Network>(
    dir_path: PathBuf,
    block_no: u64,
    rpc_url: String,
) -> anyhow::Result<Rc<RefCell<dyn Provider<N>>>> {
    Ok(Rc::new(RefCell::new(
        cache_provider::CachedRpcProvider::new(dir_path, block_no, rpc_url)?,
    )))
}

pub fn new_provider<N: Network>(
    cache_dir: Option<PathBuf>,
    block_no: u64,
    rpc_url: Option<String>,
) -> anyhow::Result<Rc<RefCell<dyn Provider<N>>>> {
    match (cache_dir, rpc_url) {
        (Some(cache_path), Some(rpc_url)) => new_cached_rpc_provider(cache_path, block_no, rpc_url),
        (Some(cache_path), None) => new_file_provider(cache_path, block_no),
        (None, Some(rpc_url)) => new_rpc_provider(rpc_url),
        (None, None) => Err(anyhow!("No cache_path or rpc_url given")),
    }
}

pub trait Provider<N: Network>: Send {
    fn save(&self) -> anyhow::Result<()>;
    fn advance(&mut self) -> anyhow::Result<()>;

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<N::BlockResponse>;
    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<N::BlockResponse>;
    fn get_block_receipts(&mut self, query: &BlockQuery)
        -> anyhow::Result<Vec<N::ReceiptResponse>>;
    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse>;
    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256>;
    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256>;
    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes>;
    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256>;
}

pub trait MutProvider<N: Network>: Provider<N> {
    fn insert_full_block(&mut self, query: BlockQuery, val: N::BlockResponse);
    fn insert_uncle_block(&mut self, query: UncleQuery, val: N::BlockResponse);
    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<N::ReceiptResponse>);
    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186AccountProofResponse);
    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256);
    fn insert_balance(&mut self, query: AccountQuery, val: U256);
    fn insert_code(&mut self, query: AccountQuery, val: Bytes);
    fn insert_storage(&mut self, query: StorageQuery, val: U256);
}

pub fn get_proofs<N: Network>(
    provider: &mut dyn Provider<N>,
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

/// A serde helper to serialize a HashMap into a vector sorted by key
pub mod ordered_map {
    use std::{collections::HashMap, hash::Hash};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Ord + Serialize,
        V: Serialize,
    {
        let mut vec: Vec<(_, _)> = map.iter().collect();
        vec.sort_unstable_by_key(|&(k, _)| k);
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Eq + Hash + Deserialize<'de>,
        V: Deserialize<'de>,
    {
        let vec = Vec::<(_, _)>::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}
