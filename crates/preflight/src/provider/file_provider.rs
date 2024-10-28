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

use super::{
    AccountQuery, BlockQuery, MutProvider, ProofQuery, Provider, StorageQuery, UncleQuery,
};
use alloy::primitives::{Bytes, U256};
use alloy::rpc::types::{Block, EIP1186AccountProofResponse, Transaction, TransactionReceipt};
use anyhow::{anyhow, Context};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::mem::replace;
use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    path::PathBuf,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FileProvider {
    #[serde(skip)]
    block_no: u64,
    #[serde(skip)]
    dir_path: PathBuf,
    #[serde(skip)]
    dirty: bool,
    #[serde(with = "ordered_map")]
    full_blocks: HashMap<BlockQuery, Block<Transaction>>,
    #[serde(with = "ordered_map")]
    uncle_blocks: HashMap<UncleQuery, Block<Transaction>>,
    #[serde(default)]
    #[serde(with = "ordered_map")]
    receipts: HashMap<BlockQuery, Vec<TransactionReceipt>>,
    #[serde(with = "ordered_map")]
    proofs: HashMap<ProofQuery, EIP1186AccountProofResponse>,
    #[serde(with = "ordered_map")]
    transaction_count: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map")]
    balance: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map")]
    code: HashMap<AccountQuery, Bytes>,
    #[serde(with = "ordered_map")]
    storage: HashMap<StorageQuery, U256>,
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
    /// Creates a new [FileProvider]. If the file exists, it will be read and
    /// deserialized. Otherwise, a new file will be created when saved.
    pub fn new(dir_path: PathBuf, block_no: u64) -> anyhow::Result<Self> {
        let file_path = Self::derive_file_path(&dir_path, block_no);
        let provider = match FileProvider::read(dir_path.clone(), block_no) {
            Ok(provider) => Ok(provider),
            Err(err) => match err.downcast_ref::<io::Error>() {
                Some(io_err) if io_err.kind() == io::ErrorKind::NotFound => {
                    // create the file and directory if it doesn't exist
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    Ok(FileProvider {
                        dir_path,
                        block_no,
                        ..Default::default()
                    })
                }
                _ => Err(err),
            },
        }?;
        Ok(provider)
    }

    fn derive_file_path(dir_path: &PathBuf, block_no: u64) -> PathBuf {
        dir_path
            .join(block_no.to_string())
            .with_extension("json.gz")
    }

    fn read(dir_path: PathBuf, block_no: u64) -> anyhow::Result<Self> {
        let file_path = Self::derive_file_path(&dir_path, block_no);
        let f = File::open(&file_path)?;
        let mut out: Self = serde_json::from_reader(GzDecoder::new(f))?;
        out.dir_path = dir_path;
        out.dirty = false;
        out.block_no = block_no;

        Ok(out)
    }
}

impl Provider for FileProvider {
    fn save(&self) -> anyhow::Result<()> {
        if self.dirty {
            let file_path = Self::derive_file_path(&self.dir_path, self.block_no);
            let f = File::create(&file_path)
                .with_context(|| format!("Failed to create '{}'", self.dir_path.display()))?;
            let mut encoder = GzEncoder::new(f, Compression::best());
            serde_json::to_writer(&mut encoder, &self)?;
            encoder.finish()?;
        }

        Ok(())
    }

    fn advance(&mut self) -> anyhow::Result<()> {
        Ok(drop(replace(
            self,
            FileProvider::new(self.dir_path.clone(), self.block_no + 1)?,
        )))
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<Block<Transaction>> {
        match self.full_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<Block<Transaction>> {
        match self.uncle_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<Vec<TransactionReceipt>> {
        match self.receipts.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse> {
        match self.proofs.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        match self.transaction_count.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        match self.balance.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes> {
        match self.code.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256> {
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

    fn insert_uncle_block(&mut self, query: UncleQuery, val: Block<Transaction>) {
        self.uncle_blocks.insert(query, val);
        self.dirty = true;
    }

    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<TransactionReceipt>) {
        self.receipts.insert(query, val);
        self.dirty = true;
    }

    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186AccountProofResponse) {
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

    fn insert_storage(&mut self, query: StorageQuery, val: U256) {
        self.storage.insert(query, val);
        self.dirty = true;
    }
}