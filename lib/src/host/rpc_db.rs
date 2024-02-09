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

use std::path::{Path, PathBuf};

use anyhow::Context;
use zeth_primitives::{
    block::Header,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence},
    Address,
};

use crate::{
    host::provider::{new_provider, BlockQuery},
    optimism::{
        batcher_db::{BatcherDb, BlockInput, MemDb},
        config::ChainConfig,
        deposits, system_config,
    },
};

fn cache_file_path(cache_path: &Path, network: &str, block_no: u64, ext: &str) -> PathBuf {
    cache_path
        .join(network)
        .join(block_no.to_string())
        .with_extension(ext)
}

fn eth_cache_path(cache: &Option<PathBuf>, block_no: u64) -> Option<PathBuf> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "ethereum", block_no, "json.gz"))
}

fn op_cache_path(cache: &Option<PathBuf>, block_no: u64) -> Option<PathBuf> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "optimism", block_no, "json.gz"))
}

pub struct RpcDb {
    deposit_contract: Address,
    system_config_contract: Address,
    eth_rpc_url: Option<String>,
    op_rpc_url: Option<String>,
    cache: Option<PathBuf>,
    mem_db: MemDb,
}

impl RpcDb {
    pub fn new(
        config: &ChainConfig,
        eth_rpc_url: Option<String>,
        op_rpc_url: Option<String>,
        cache: Option<PathBuf>,
    ) -> Self {
        RpcDb {
            deposit_contract: config.deposit_contract,
            system_config_contract: config.system_config_contract,
            eth_rpc_url,
            op_rpc_url,
            cache,
            mem_db: MemDb::new(),
        }
    }

    pub fn get_mem_db(self) -> MemDb {
        self.mem_db
    }
}

impl BatcherDb for RpcDb {
    fn validate(&self, _: &ChainConfig) -> anyhow::Result<()> {
        Ok(())
    }

    fn get_full_op_block(
        &mut self,
        block_no: u64,
    ) -> anyhow::Result<BlockInput<OptimismTxEssence>> {
        let mut provider = new_provider(
            op_cache_path(&self.cache, block_no),
            self.op_rpc_url.clone(),
        )
        .context("failed to create provider")?;
        let block = {
            let ethers_block = provider.get_full_block(&BlockQuery { block_no })?;
            BlockInput {
                block_header: ethers_block.clone().try_into().unwrap(),
                transactions: ethers_block
                    .transactions
                    .into_iter()
                    .map(|tx| tx.try_into().unwrap())
                    .collect(),
                receipts: None,
            }
        };
        self.mem_db.full_op_block.insert(block_no, block.clone());
        provider.save()?;
        Ok(block)
    }

    fn get_op_block_header(&mut self, block_no: u64) -> anyhow::Result<Header> {
        let mut provider = new_provider(
            op_cache_path(&self.cache, block_no),
            self.op_rpc_url.clone(),
        )?;
        let header: Header = provider
            .get_partial_block(&BlockQuery { block_no })?
            .try_into()?;
        self.mem_db.op_block_header.insert(block_no, header.clone());
        provider.save()?;
        Ok(header)
    }

    fn get_full_eth_block(
        &mut self,
        block_no: u64,
    ) -> anyhow::Result<&BlockInput<EthereumTxEssence>> {
        let query = BlockQuery { block_no };
        let mut provider = new_provider(
            eth_cache_path(&self.cache, block_no),
            self.eth_rpc_url.clone(),
        )?;
        let block = {
            let ethers_block = provider.get_full_block(&query)?;
            let block_header: Header = ethers_block.clone().try_into().unwrap();
            // include receipts when needed
            let can_contain_deposits =
                deposits::can_contain(&self.deposit_contract, &block_header.logs_bloom);
            let can_contain_config =
                system_config::can_contain(&self.system_config_contract, &block_header.logs_bloom);
            let receipts = if can_contain_config || can_contain_deposits {
                let receipts = provider.get_block_receipts(&query)?;
                Some(
                    receipts
                        .into_iter()
                        .map(|receipt| receipt.try_into())
                        .collect::<anyhow::Result<Vec<_>, _>>()
                        .context("invalid receipt")?,
                )
            } else {
                None
            };
            BlockInput {
                block_header,
                transactions: ethers_block
                    .transactions
                    .into_iter()
                    .map(|tx| tx.try_into().unwrap())
                    .collect(),
                receipts,
            }
        };
        self.mem_db.full_eth_block.insert(block_no, block);
        provider.save()?;
        self.mem_db.get_full_eth_block(block_no)
    }
}
