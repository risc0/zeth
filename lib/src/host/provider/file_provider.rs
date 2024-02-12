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

use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use ethers_core::types::{
    Block, Bytes, EIP1186ProofResponse, Transaction, TransactionReceipt, H256, U256,
};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};

use super::{AccountQuery, BlockQuery, MutProvider, ProofQuery, Provider, StorageQuery};

#[derive(Clone, Default, Deserialize, Serialize)]
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
    /// Creates a new [FileProvider]. If the file exists, it will be read and
    /// deserialized. Otherwise, a new file will be created when saved.
    pub fn new(file_path: PathBuf) -> Result<Self> {
        match FileProvider::read(file_path.clone()) {
            Ok(provider) => Ok(provider),
            Err(err) => match err.downcast_ref::<io::Error>() {
                Some(io_err) if io_err.kind() == io::ErrorKind::NotFound => {
                    // create the file and directory if it doesn't exist
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    Ok(FileProvider {
                        file_path,
                        ..Default::default()
                    })
                }
                _ => Err(err),
            },
        }
    }

    fn read(file_path: PathBuf) -> Result<Self> {
        let f = File::open(&file_path)?;
        let mut out: Self = serde_json::from_reader(GzDecoder::new(f))?;
        out.file_path = file_path;
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
        if self.dirty {
            let f = File::create(&self.file_path)
                .with_context(|| format!("Failed to create '{}'", self.file_path.display()))?;
            let mut encoder = GzEncoder::new(f, Compression::best());
            serde_json::to_writer(&mut encoder, &self)?;
            encoder.finish()?;
        }

        Ok(())
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
