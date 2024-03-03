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

use anyhow::{anyhow, Result};
use ethers_core::types::Log;
use ethers_core::types::{
    Block, Bytes, Filter, Transaction, TransactionReceipt, H256, U256,
};
use ethers_providers::{Http as ethers_Http, Middleware, RetryClient};
use alloy_rpc_types::{BlockId, EIP1186AccountProofResponse};
use alloy_providers::provider::HttpProvider;
use alloy_providers::provider::TempProvider;
use alloy_transport_http::Http;
use hex::FromHex;
use url::Url;
use log::info;

use super::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery};
use crate::host::provider::LogsQuery;

pub struct RpcProvider {
    ethers_provider: ethers_providers::Provider<RetryClient<ethers_Http>>,
    alloy_provider: HttpProvider,
    tokio_handle: tokio::runtime::Handle,
}

impl RpcProvider {
    pub fn new(rpc_url: String) -> Result<Self> {
        //TODO(Brecht): switch to alloy provider for everything
        let ethers_provider =
            ethers_providers::Provider::<RetryClient<ethers_Http>>::new_client(&rpc_url, 3, 500)?;

        let alloy_http = Http::new(Url::parse(&rpc_url).expect("invalid rpc url"));
        let alloy_provider: HttpProvider = HttpProvider::new(alloy_http);

        let tokio_handle = tokio::runtime::Handle::current();

        Ok(RpcProvider {
            ethers_provider,
            alloy_provider,
            tokio_handle,
        })
    }
}

impl Provider for RpcProvider {
    fn save(&self) -> Result<()> {
        Ok(())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>> {
        info!("Querying RPC for full block: {query:?}");

        let response = self
            .tokio_handle
            .block_on(async { self.ethers_provider.get_block_with_txs(query.block_no).await })?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {query:?}")),
        }
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>> {
        info!("Querying RPC for partial block: {query:?}");

        let response = self
            .tokio_handle
            .block_on(async { self.ethers_provider.get_block(query.block_no).await })?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {query:?}")),
        }
    }

    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>> {
        info!("Querying RPC for block receipts: {query:?}");

        let response = self
            .tokio_handle
            .block_on(async { self.ethers_provider.get_block_receipts(query.block_no).await })?;

        Ok(response)
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186AccountProofResponse> {
        info!("Querying RPC for inclusion proof: {query:?}");

        let out: EIP1186AccountProofResponse = self.tokio_handle.block_on(async {
            self.alloy_provider
                .get_proof(
                    zeth_primitives::Address::from_slice(&query.address.as_bytes()),
                    query.indices.iter().cloned().map(|v| alloy_primitives::FixedBytes(*v.as_fixed_bytes())).collect(),
                    Some(BlockId::from(query.block_no)),
                )
                .await
        })?;

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        info!("Querying RPC for transaction count: {query:?}");

        let out = self.tokio_handle.block_on(async {
            self.ethers_provider
                .get_transaction_count(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        info!("Querying RPC for balance: {query:?}");

        let out = self.tokio_handle.block_on(async {
            self.ethers_provider
                .get_balance(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        info!("Querying RPC for code: {query:?}");

        let out = self.tokio_handle.block_on(async {
            self.ethers_provider
                .get_code(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256> {
        info!("Querying RPC for storage: {query:?}");

        let out = self.tokio_handle.block_on(async {
            self.ethers_provider
                .get_storage_at(query.address, query.index, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    #[cfg(feature = "taiko")]
    fn get_logs(&mut self, query: &LogsQuery) -> Result<Vec<Log>> {
        info!("Querying RPC for logs: {query:?}");

        let out = self.tokio_handle.block_on(async {
            self.ethers_provider
                .get_logs(
                    &Filter::new()
                        .address(query.address)
                        .from_block(query.from_block)
                        .to_block(query.to_block),
                )
                .await
        })?;

        Ok(out)
    }

    #[cfg(feature = "taiko")]
    fn get_transaction(&mut self, query: &super::TxQuery) -> Result<Transaction> {
        info!("Querying RPC for tx: {query:?}");
        let out = self
            .tokio_handle
            .block_on(async { self.ethers_provider.get_transaction(query.tx_hash).await })?;
        match out {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {query:?}")),
        }
    }
}
