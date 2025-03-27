// Copyright 2023, 2024 RISC Zero, Inc.
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

use super::{ordered_map, MutProvider, Provider};
use crate::provider::query::{
    AccountQuery, BlockQuery, NextAccountQuery, NextSlotQuery, PreimageQuery, ProofQuery,
    StorageQuery, UncleQuery,
};
use alloy::network::Network;
use alloy::primitives::{Address, Bytes, U256};
use alloy::rpc::types::EIP1186AccountProofResponse;
use anyhow::{anyhow, Context};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use reth_chainspec::NamedChain;
use serde::{Deserialize, Serialize};
use std::mem::replace;
use std::path::Path;
use std::{
    collections::HashMap,
    fs::{self, File},
    io,
    path::PathBuf,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileProvider<N: Network> {
    #[serde(skip)]
    chain_id: u64,
    #[serde(skip)]
    block_no: u64,
    #[serde(skip)]
    dir_path: PathBuf,
    #[serde(skip)]
    dirty: bool,
    #[serde(default)]
    client_version: String,
    #[serde(with = "ordered_map", default)]
    full_blocks: HashMap<BlockQuery, N::BlockResponse>,
    #[serde(with = "ordered_map", default)]
    uncle_blocks: HashMap<UncleQuery, N::BlockResponse>,
    #[serde(with = "ordered_map", default)]
    receipts: HashMap<BlockQuery, Vec<N::ReceiptResponse>>,
    #[serde(with = "ordered_map", default)]
    proofs: HashMap<ProofQuery, EIP1186AccountProofResponse>,
    #[serde(with = "ordered_map", default)]
    transaction_count: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map", default)]
    balance: HashMap<AccountQuery, U256>,
    #[serde(with = "ordered_map", default)]
    code: HashMap<AccountQuery, Bytes>,
    #[serde(with = "ordered_map", default)]
    storage: HashMap<StorageQuery, U256>,
    #[serde(with = "ordered_map", default)]
    preimages: HashMap<PreimageQuery, Bytes>,
    #[serde(with = "ordered_map", default)]
    next_accounts: HashMap<NextAccountQuery, Address>,
    #[serde(with = "ordered_map", default)]
    next_slots: HashMap<NextSlotQuery, U256>,
}

impl<N: Network> FileProvider<N> {
    /// Creates a new [FileProvider]. If the file exists, it will be read and
    /// deserialized. Otherwise, a new file will be created when saved.
    pub fn new(dir_path: PathBuf, block_no: u64, chain_id: u64) -> anyhow::Result<Self> {
        let file_path = Self::derive_file_path(&dir_path, block_no, chain_id);
        let provider = match FileProvider::read(dir_path.clone(), block_no, chain_id) {
            Ok(provider) => Ok(provider),
            Err(err) => match err.downcast_ref::<io::Error>() {
                Some(io_err) if io_err.kind() == io::ErrorKind::NotFound => {
                    // create the file and directory if it doesn't exist
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    Ok(FileProvider {
                        chain_id,
                        block_no,
                        dir_path,
                        client_version: Default::default(),
                        dirty: false,
                        full_blocks: Default::default(),
                        uncle_blocks: Default::default(),
                        receipts: Default::default(),
                        proofs: Default::default(),
                        transaction_count: Default::default(),
                        balance: Default::default(),
                        code: Default::default(),
                        storage: Default::default(),
                        preimages: Default::default(),
                        next_accounts: Default::default(),
                        next_slots: Default::default(),
                    })
                }
                _ => Err(err),
            },
        }?;
        Ok(provider)
    }

    fn derive_file_path(dir_path: &Path, block_no: u64, chain_id: u64) -> PathBuf {
        dir_path
            .join(format!("{chain_id}-{block_no}"))
            .with_extension("json.gz")
    }

    fn read(dir_path: PathBuf, block_no: u64, chain_id: u64) -> anyhow::Result<Self> {
        let file_path = Self::derive_file_path(&dir_path, block_no, chain_id);
        let file = File::open(&file_path)?;
        let decompressed = GzDecoder::new(file);
        let mut out: Self = serde_json::from_reader(decompressed)?;
        out.dir_path = dir_path;
        out.dirty = false;
        out.block_no = block_no;
        out.chain_id = chain_id;

        Ok(out)
    }
}

impl<N: Network> Provider<N> for FileProvider<N> {
    fn save(&self) -> anyhow::Result<()> {
        if self.dirty {
            let file_path = Self::derive_file_path(&self.dir_path, self.block_no, self.chain_id);
            let f = File::create(&file_path)
                .with_context(|| format!("Failed to create '{}'", self.dir_path.display()))?;
            let mut encoder = GzEncoder::new(f, Compression::best());
            serde_json::to_writer(&mut encoder, &self)?;
            encoder.finish()?;
        }

        Ok(())
    }

    fn advance(&mut self) -> anyhow::Result<()> {
        self.reset(self.block_no + 1)
    }

    fn reset(&mut self, block_no: u64) -> anyhow::Result<()> {
        drop(replace(
            self,
            FileProvider::new(self.dir_path.clone(), block_no, self.chain_id)?,
        ));
        Ok(())
    }

    fn get_client_version(&mut self) -> anyhow::Result<String> {
        Ok(self.client_version.clone())
    }

    fn get_chain(&mut self) -> anyhow::Result<NamedChain> {
        Ok(NamedChain::try_from(self.chain_id).unwrap())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<N::BlockResponse> {
        match self.full_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<N::BlockResponse> {
        match self.uncle_blocks.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<Vec<N::ReceiptResponse>> {
        match self.receipts.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse> {
        match self.proofs.get(query) {
            Some(val) => Ok(val.clone()),
            None => {
                // return result if a query that targets a superset of keys is found
                for (stored_query, proof) in self.proofs.iter() {
                    if query.block_no != stored_query.block_no
                        || query.address != stored_query.address
                    {
                        continue;
                    }
                    if query
                        .indices
                        .iter()
                        .all(|idx| stored_query.indices.contains(idx))
                    {
                        return Ok(proof.clone());
                    }
                }
                Err(anyhow!("No data for {:?}", query))
            }
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

    fn get_preimage(&mut self, query: &PreimageQuery) -> anyhow::Result<Bytes> {
        match self.preimages.get(query) {
            Some(val) => Ok(val.clone()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_next_account(&mut self, query: &NextAccountQuery) -> anyhow::Result<Address> {
        match self.next_accounts.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_next_slot(&mut self, query: &NextSlotQuery) -> anyhow::Result<U256> {
        match self.next_slots.get(query) {
            Some(val) => Ok(*val),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }
}

impl<N: Network> MutProvider<N> for FileProvider<N> {
    fn insert_client_version(&mut self, version: String) {
        self.client_version = version;
        self.dirty = true;
    }

    fn insert_chain(&mut self, chain: NamedChain) {
        assert_eq!(self.chain_id, chain as u64);
    }

    fn insert_full_block(&mut self, query: BlockQuery, val: N::BlockResponse) {
        self.full_blocks.insert(query, val);
        self.dirty = true;
    }

    fn insert_uncle_block(&mut self, query: UncleQuery, val: N::BlockResponse) {
        self.uncle_blocks.insert(query, val);
        self.dirty = true;
    }

    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<N::ReceiptResponse>) {
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

    fn insert_preimage(&mut self, query: PreimageQuery, val: Bytes) {
        self.preimages.insert(query, val);
        self.dirty = true
    }

    fn insert_next_account(&mut self, query: NextAccountQuery, val: Address) {
        self.next_accounts.insert(query, val);
        self.dirty = true;
    }

    fn insert_next_slot(&mut self, query: NextSlotQuery, val: U256) {
        self.next_slots.insert(query, val);
        self.dirty = true;
    }
}
