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

use crate::provider::file_provider::FileProvider;
use crate::provider::rpc_provider::RpcProvider;
use crate::provider::*;
use anyhow::Context;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct CachedRpcProvider<N: Network> {
    cache: FileProvider<N>,
    rpc: RpcProvider<N>,
}

impl<N: Network> CachedRpcProvider<N> {
    pub fn new(
        cache_dir: PathBuf,
        block_no: u64,
        rpc_url: String,
        chain_id: Option<u64>,
    ) -> anyhow::Result<Self> {
        let mut rpc = RpcProvider::new(rpc_url).context("failed to init RPC")?;
        let chain_id = chain_id.unwrap_or_else(|| rpc.get_chain().unwrap() as u64);
        let cache =
            FileProvider::new(cache_dir, block_no, chain_id).context("failed to init cache")?;

        Ok(CachedRpcProvider { cache, rpc })
    }
}

impl<N: Network> Provider<N> for CachedRpcProvider<N> {
    fn save(&self) -> anyhow::Result<()> {
        self.cache.save()
    }

    fn advance(&mut self) -> anyhow::Result<()> {
        self.cache.advance()
    }

    fn reset(&mut self, block_no: u64) -> anyhow::Result<()> {
        self.cache.reset(block_no)
    }

    fn get_client_version(&mut self) -> anyhow::Result<String> {
        if let Ok(cache_out) = self.cache.get_client_version() {
            if !cache_out.is_empty() {
                return Ok(cache_out);
            }
        }

        let out = self.rpc.get_client_version()?;
        self.cache.insert_client_version(out.clone());

        Ok(out)
    }

    fn get_chain(&mut self) -> anyhow::Result<NamedChain> {
        let cache_out = self.cache.get_chain();
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_chain()?;
        self.cache.insert_chain(out);

        Ok(out)
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<N::BlockResponse> {
        let cache_out = self.cache.get_full_block(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_full_block(query)?;
        self.cache.insert_full_block(query.clone(), out.clone());

        Ok(out)
    }

    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<N::BlockResponse> {
        let cache_out = self.cache.get_uncle_block(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_uncle_block(query)?;
        self.cache.insert_uncle_block(query.clone(), out.clone());

        Ok(out)
    }

    fn get_block_receipts(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<Vec<N::ReceiptResponse>> {
        let cache_out = self.cache.get_block_receipts(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_block_receipts(query)?;
        self.cache.insert_block_receipts(query.clone(), out.clone());

        Ok(out)
    }

    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse> {
        let cache_out = self.cache.get_proof(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_proof(query)?;
        self.cache.insert_proof(query.clone(), out.clone());

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        let cache_out = self.cache.get_transaction_count(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_transaction_count(query)?;
        self.cache.insert_transaction_count(query.clone(), out);

        Ok(out)
    }

    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        let cache_out = self.cache.get_balance(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_balance(query)?;
        self.cache.insert_balance(query.clone(), out);

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes> {
        let cache_out = self.cache.get_code(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_code(query)?;
        self.cache.insert_code(query.clone(), out.clone());

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256> {
        let cache_out = self.cache.get_storage(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_storage(query)?;
        self.cache.insert_storage(query.clone(), out);

        Ok(out)
    }

    fn get_preimage(&mut self, query: &PreimageQuery) -> anyhow::Result<Bytes> {
        let cache_out = self.cache.get_preimage(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_preimage(query)?;
        self.cache.insert_preimage(query.clone(), out.clone());

        Ok(out)
    }

    fn get_next_account(&mut self, query: &NextAccountQuery) -> anyhow::Result<Address> {
        let cache_out = self.cache.get_next_account(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_next_account(query)?;
        self.cache.insert_next_account(query.clone(), out);

        Ok(out)
    }

    fn get_next_slot(&mut self, query: &NextSlotQuery) -> anyhow::Result<U256> {
        let cache_out = self.cache.get_next_slot(query);
        if cache_out.is_ok() {
            return cache_out;
        }

        let out = self.rpc.get_next_slot(query)?;
        self.cache.insert_next_slot(query.clone(), out);

        Ok(out)
    }
}
