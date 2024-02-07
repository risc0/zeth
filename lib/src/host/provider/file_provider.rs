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

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use ethers_core::types::{
    Block, Bytes, EIP1186ProofResponse, Transaction, TransactionReceipt, H256, U256,
};
use serde::{Deserialize, Serialize};

use super::{AccountQuery, BlockQuery, MutProvider, ProofQuery, Provider, StorageQuery};

#[derive(Clone, Deserialize, Serialize)]
pub struct FileProvider {
    #[serde(skip)]
    file_path: PathBuf,
    #[serde(skip)]
    dirty: bool,
    #[serde(with = "ordered_map")]
    full_blocks: HashMap<BlockQuery, Block<Transaction>>,
    #[serde(with = "ordered_map")]
    partial_blocks: HashMap<BlockQuery, Block<H256>>,
    #[serde(default)]
    #[serde(with = "ordered_map")]
    receipts: HashMap<BlockQuery, Vec<TransactionReceipt>>,
    #[serde(with = "ordered_map")]
    proofs: HashMap<ProofQuery, EIP1186ProofResponse>,
    #[serde(with = "ordered_map")]
    transaction_count: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map")]
    balance: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map")]
    code: HashMap<AccountQuery, Bytes>,
    #[serde(with = "ordered_map")]
    storage: HashMap<StorageQuery, H256>,
}

/// A serde helper to serialize a HashMap into a vector sorted by key
mod ordered_map {
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

impl FileProvider {
    pub fn empty(file_path: PathBuf) -> Self {
        FileProvider {
            file_path,
            dirty: false,
            full_blocks: HashMap::new(),
            partial_blocks: HashMap::new(),
            receipts: HashMap::new(),
            proofs: HashMap::new(),
            transaction_count: HashMap::new(),
            balance: HashMap::new(),
            code: HashMap::new(),
            storage: HashMap::new(),
        }
    }

    pub fn from_file(file_path: &PathBuf) -> Result<Self> {
        let mut buf = vec![];
        let mut decoder = flate2::read::GzDecoder::new(File::open(file_path)?);
        decoder.read_to_end(&mut buf)?;

        let mut out: Self = serde_json::from_slice(&buf[..])?;

        out.file_path = file_path.clone();
        out.dirty = false;
        Ok(out)
    }

    pub fn save_to_file(&self, file_path: &Path) -> Result<()> {
        if self.dirty {
            let mut encoder = flate2::write::GzEncoder::new(
                File::create(file_path)
                    .with_context(|| format!("Failed to create '{}'", file_path.display()))?,
                flate2::Compression::best(),
            );
            encoder.write_all(&serde_json::to_vec(self)?)?;
            encoder.finish()?;
        }

        Ok(())
    }
}

impl Provider for FileProvider {
    fn save(&self) -> Result<()> {
        self.save_to_file(&self.file_path)
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>> {
        match self.full_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>> {
        match self.partial_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>> {
        match self.receipts.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse> {
        match self.proofs.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        match self.transaction_count.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        match self.balance.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        match self.code.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256> {
        match self.storage.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }
}

impl MutProvider for FileProvider {
    fn insert_full_block(&mut self, query: BlockQuery, val: Block<Transaction>) {
        self.full_blocks.insert(query, val);
        self.dirty = true;
    }

    fn insert_partial_block(&mut self, query: BlockQuery, val: Block<H256>) {
        self.partial_blocks.insert(query, val);
        self.dirty = true;
    }

    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<TransactionReceipt>) {
        self.receipts.insert(query, val);
        self.dirty = true;
    }

    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186ProofResponse) {
        self.proofs.insert(query, val);
        self.dirty = true;
    }

    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256) {
        self.transaction_count.insert(query, val);
        self.dirty = true;
    }

    fn insert_balance(&mut self, query: AccountQuery, val: U256) {
        self.balance.insert(query, val);
        self.dirty = true;
    }

    fn insert_code(&mut self, query: AccountQuery, val: Bytes) {
        self.code.insert(query, val);
        self.dirty = true;
    }

    fn insert_storage(&mut self, query: StorageQuery, val: H256) {
        self.storage.insert(query, val);
        self.dirty = true;
    }
}
