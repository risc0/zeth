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

use crate::provider::*;
use alloy::providers::{Provider as AlloyProvider, ReqwestProvider};
use anyhow::anyhow;
use log::debug;
use std::future::IntoFuture;

#[derive(Clone, Debug)]
pub struct RpcProvider {
    http_client: ReqwestProvider,
    tokio_handle: tokio::runtime::Handle,
}

impl RpcProvider {
    pub fn new(rpc_url: String) -> anyhow::Result<Self> {
        let http_client = ReqwestProvider::new_http(rpc_url.parse()?);
        let tokio_handle = tokio::runtime::Handle::current();

        Ok(RpcProvider {
            http_client,
            tokio_handle,
        })
    }
}

impl Provider for RpcProvider {
    fn save(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<Block<Transaction>> {
        debug!("Querying RPC for full block: {:?}", query);

        let response = self.tokio_handle.block_on(
            self.http_client
                .get_block_by_number(query.block_no.into(), true),
        )?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<Block<Transaction>> {
        debug!("Querying RPC for uncle block: {:?}", query);

        let response = self.tokio_handle.block_on(
            self.http_client
                .get_uncle(query.uncle_hash.into(), query.index_number.into()),
        )?;

        match response {
            Some(out) => Ok(out.into()),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<Vec<TransactionReceipt>> {
        debug!("Querying RPC for block receipts: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(self.http_client.get_block_receipts(query.block_no.into()))?
            .unwrap();

        Ok(response)
    }

    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse> {
        debug!("Querying RPC for inclusion proof: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_proof(query.address, query.indices.iter().cloned().collect())
                .number(query.block_no)
                .into_future(),
        )?;

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        debug!("Querying RPC for transaction count: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_transaction_count(query.address)
                .number(query.block_no)
                .into_future(),
        )?;

        Ok(U256::from(out))
    }

    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        debug!("Querying RPC for balance: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_balance(query.address)
                .number(query.block_no)
                .into_future(),
        )?;

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes> {
        debug!("Querying RPC for code: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_code_at(query.address)
                .number(query.block_no)
                .into_future(),
        )?;

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256> {
        debug!("Querying RPC for storage: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_storage_at(query.address, query.index)
                .number(query.block_no)
                .into_future(),
        )?;

        Ok(out)
    }
}
