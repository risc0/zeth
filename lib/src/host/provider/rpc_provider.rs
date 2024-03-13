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

use alloy::providers::provider::{HttpProvider, TempProvider};
use alloy::rpc::types::eth::{Block, EIP1186AccountProofResponse, TransactionReceipt};
use anyhow::{anyhow, Result};
use log::debug;
use zeth_primitives::{Bytes, U256};

use super::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery};

pub struct RpcProvider {
    http_client: HttpProvider,
    tokio_handle: tokio::runtime::Handle,
}

impl RpcProvider {
    pub fn new(rpc_url: String) -> Result<Self> {
        let http_client: HttpProvider = rpc_url.try_into()?;
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

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block> {
        debug!("Querying RPC for full block: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(self.http_client.get_block(query.block_no.into(), true))?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block> {
        debug!("Querying RPC for partial block: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(self.http_client.get_block(query.block_no.into(), false))?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>> {
        debug!("Querying RPC for block receipts: {:?}", query);

        let response = self
            .tokio_handle
            .block_on(self.http_client.get_block_receipts(query.block_no.into()))?;

        Ok(response.unwrap_or_default())
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186AccountProofResponse> {
        debug!("Querying RPC for inclusion proof: {:?}", query);

        let out = self.tokio_handle.block_on(self.http_client.get_proof(
            query.address,
            query.indices.iter().cloned().collect(),
            Some(query.block_no.into()),
        ))?;

        Ok(out)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256> {
        debug!("Querying RPC for transaction count: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_transaction_count(query.address, Some(query.block_no.into())),
        )?;

        Ok(out)
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256> {
        debug!("Querying RPC for balance: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_balance(query.address, Some(query.block_no.into())),
        )?;

        Ok(out)
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes> {
        debug!("Querying RPC for code: {:?}", query);

        let out = self.tokio_handle.block_on(
            self.http_client
                .get_code_at(query.address, Some(query.block_no.into())),
        )?;

        Ok(out)
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<U256> {
        debug!("Querying RPC for storage: {:?}", query);

        let out = self.tokio_handle.block_on(self.http_client.get_storage_at(
            query.address,
            query.index,
            Some(query.block_no.into()),
        ))?;

        Ok(out)
    }
}
