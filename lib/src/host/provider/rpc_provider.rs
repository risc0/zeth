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
use ethers_core::types::{
    Block, Bytes, EIP1186ProofResponse, Transaction, TransactionReceipt, H256, U256,
};
use ethers_providers::{Http, Middleware, RetryClient};
use log::info;
#[cfg(feature = "taiko")]
use zeth_primitives::taiko::BlockProposed;

use super::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery};

pub struct RpcProvider {
    http_client: ethers_providers::Provider<RetryClient<Http>>,
    tokio_handle: tokio::runtime::Handle,
}

impl RpcProvider {
    pub fn new(rpc_url: String) -> Result<Self> {
        let http_client =
            ethers_providers::Provider::<RetryClient<Http>>::new_client(&rpc_url, 3, 500)?;
        let tokio_handle = tokio::runtime::Handle::current();

        Ok(RpcProvider {
            http_client,
            tokio_handle,
        })
    }
}

impl Provider for RpcProvider {
    fn save(&self) -> Result<()> {
        Ok(())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>> {
        info!("Querying RPC for full block: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(async { self.http_client.get_block_with_txs(query.block_no).await })?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>> {
        info!("Querying RPC for partial block: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(async { self.http_client.get_block(query.block_no).await })?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>> {
        info!("Querying RPC for block receipts: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(async { self.http_client.get_block_receipts(query.block_no).await })?;

        Ok(response)
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse> {
        info!("Querying RPC for inclusion proof: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            self.http_client
                .get_proof(
                    query.address,
                    query.indices.iter().cloned().collect(),
                    Some(query.block_no.into()),
                )
                .await
        })?;

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        info!("Querying RPC for transaction count: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            self.http_client
                .get_transaction_count(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        info!("Querying RPC for balance: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            self.http_client
                .get_balance(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        info!("Querying RPC for code: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            self.http_client
                .get_code(query.address, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256> {
        info!("Querying RPC for storage: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            self.http_client
                .get_storage_at(query.address, query.index, Some(query.block_no.into()))
                .await
        })?;

        Ok(out)
    }

    #[cfg(feature = "taiko")]
    fn get_propose(&mut self, query: &super::ProposeQuery) -> Result<(Transaction, BlockProposed)> {
        use revm::primitives::U256;
        info!("Querying RPC for propose: {:?}", query);
        let filter = Filter::new()
            .address(query.l1_contract)
            .from_block(query.l1_block_no)
            .to_block(query.l1_block_no);
        let logs = self
            .tokio_handle
            .block_on(async { self.http_client.get_logs(&filter).await })?;
        let result = taiko::filter_propose_block_event(&logs, U256::from(query.l2_block_no))?;
        let (tx_hash, block_proposed) =
            result.ok_or_else(|| anyhow!("No propose block event for {:?}", query))?;
        let response = self
            .tokio_handle
            .block_on(async { self.http_client.get_transaction(tx_hash).await })?;
        match response {
            Some(out) => Ok((out, block_proposed)),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    #[cfg(feature = "taiko")]
    /// get 256 blocks one time to reduce the fetch time cost
    fn batch_get_partial_blocks(&mut self, query: &BlockQuery) -> Result<Vec<Block<H256>>> {
        info!("Querying RPC for partial blocks: {:?}", query);

        let out = self.tokio_handle.block_on(async {
            use ethers_core::utils;
            let id = utils::serialize(&query.block_no);
            self.http_client
                .request("taiko_getL2ParentHeaders", [id])
                .await
        })?;
        Ok(out)
    }
}

#[cfg(feature = "taiko")]
pub mod taiko {
    use alloy_sol_types::{SolEvent, TopicList};
    use revm::primitives::U256;
    use zeth_primitives::{ethers::from_ethers_h256, taiko::BlockProposed};

    use super::*;
    pub fn filter_propose_block_event(
        logs: &[Log],
        block_id: U256,
    ) -> Result<Option<(H256, BlockProposed)>> {
        for log in logs {
            if log.topics.len() != <<BlockProposed as SolEvent>::TopicList as TopicList>::COUNT {
                continue;
            }
            if from_ethers_h256(log.topics[0]) != BlockProposed::SIGNATURE_HASH {
                continue;
            }
            let topics = log.topics.iter().map(|topic| from_ethers_h256(*topic));
            let block_proposed = BlockProposed::decode_log(topics, &log.data, false)
                .map_err(|e| anyhow!(e.to_string()))
                .with_context(|| "decode log failed")?;
            if block_proposed.blockId == block_id {
                return Ok(log.transaction_hash.map(|h| (h, block_proposed)));
            }
        }
        Ok(None)
    }
}
