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

use crate::provider::query::{AccountRangeQueryResponse, StorageRangeQueryResponse};
use crate::provider::*;
use alloy::network::{BlockResponse, HeaderResponse, Network};
use alloy::providers::{Provider as AlloyProvider, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::transports::{
    http::{Client, Http},
    layers::{RetryBackoffLayer, RetryBackoffService},
};
use anyhow::anyhow;
use log::{debug, error};
use std::future::IntoFuture;

#[derive(Clone, Debug)]
pub struct RpcProvider<N: Network> {
    http_client: RootProvider<RetryBackoffService<Http<Client>>, N>,
    tokio_handle: tokio::runtime::Handle,
}

impl<N: Network> RpcProvider<N> {
    pub fn new(rpc_url: String) -> anyhow::Result<Self> {
        let retry_layer = RetryBackoffLayer::new(100, 50, 300);

        let client = RpcClient::builder()
            .layer(retry_layer)
            .http(rpc_url.parse()?);
        let http_client = ProviderBuilder::new().network().on_client(client);

        let tokio_handle = tokio::runtime::Handle::current();

        Ok(RpcProvider {
            http_client,
            tokio_handle,
        })
    }
}

impl<N: Network> Provider<N> for RpcProvider<N> {
    fn save(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn advance(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self, _block_number: u64) -> anyhow::Result<()> {
        Ok(())
    }

    fn get_client_version(&mut self) -> anyhow::Result<String> {
        debug!("Getting rpc client version");

        Ok(self
            .tokio_handle
            .block_on(self.http_client.get_client_version())?)
    }

    fn get_chain(&mut self) -> anyhow::Result<NamedChain> {
        debug!("Querying RPC for chain id");

        let response = self
            .tokio_handle
            .block_on(self.http_client.get_chain_id())?;

        Ok(NamedChain::try_from(response).expect("Unknown chain id"))
    }

    fn get_full_block(&mut self, query: &BlockQuery) -> anyhow::Result<N::BlockResponse> {
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

    fn get_uncle_block(&mut self, query: &UncleQuery) -> anyhow::Result<N::BlockResponse> {
        debug!("Querying RPC for uncle block: {:?}", query);

        let response = self.tokio_handle.block_on(
            self.http_client
                .get_uncle(query.block_no.into(), query.uncle_index),
        )?;

        match response {
            Some(out) => Ok(out),
            None => Err(anyhow!("No data for {:?}", query)),
        }
    }

    fn get_block_receipts(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<Vec<N::ReceiptResponse>> {
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

    fn get_preimage(&mut self, query: &PreimageQuery) -> anyhow::Result<Bytes> {
        debug!("Querying RPC for preimage: {:?}", query);

        match self.tokio_handle.block_on(
            self.http_client
                .client()
                .request("debug_preimage", (query.digest.to_string(),))
                .into_future(),
        ) {
            Ok(out) => return Ok(out),
            Err(e) => {
                error!("debug_preimage: {e}");
            }
        };

        match self.tokio_handle.block_on(
            self.http_client
                .client()
                .request("debug_dbGet", (query.digest.to_string(),))
                .into_future(),
        ) {
            Ok(out) => Ok(out),
            Err(e) => {
                error!("debug_dbGet: {e}");
                anyhow::bail!(e);
            }
        }
    }

    fn get_next_account(&mut self, query: &AccountRangeQuery) -> anyhow::Result<Address> {
        let out: AccountRangeQueryResponse = match self.tokio_handle.block_on(
            self.http_client
                .client()
                .request(
                    "debug_accountRange",
                    (
                        format!("{:066x}", query.block_no),
                        format!("{}", query.start),
                        query.max_results,
                        query.no_code,
                        query.no_storage,
                        query.incompletes,
                    ),
                )
                .into_future(),
        ) {
            Ok(out) => out,
            Err(e) => {
                error!("debug_accountRange: {e}");
                anyhow::bail!(e)
            }
        };

        Ok(*out.accounts.keys().next().unwrap())
    }

    fn get_next_slot(&mut self, query: &StorageRangeQuery) -> anyhow::Result<U256> {
        let block = self.get_full_block(&BlockQuery {
            block_no: query.block_no,
        })?;
        let hash = block.header().hash();

        let out: StorageRangeQueryResponse = match self.tokio_handle.block_on(
            self.http_client
                .client()
                .request(
                    "debug_storageRangeAt",
                    (
                        // format!("{:#066x}", query.block_no),
                        format!("{hash}"),
                        query.tx_index,
                        query.address,
                        format!("{}", query.start),
                        query.max_results,
                    ),
                )
                .into_future(),
        ) {
            Ok(out) => out,
            Err(e) => {
                error!("debug_storageRangeAt: {e}");
                anyhow::bail!(e)
            }
        };

        Ok(out.storage.values().next().unwrap().key)
    }
}
