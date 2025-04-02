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

use crate::provider::*;
use alloy::eips::BlockId;
use alloy::network::{BlockResponse, HeaderResponse, Network};
use alloy::providers::{Provider as AlloyProvider, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::transports::{
    http::{Client, Http},
    layers::{RetryBackoffLayer, RetryBackoffService},
    TransportResult,
};
use anyhow::{anyhow, ensure, Context};
use log::{debug, error};
use std::future::IntoFuture;

#[derive(Clone, Debug)]
pub struct RpcProvider<N: Network> {
    http_client: RootProvider<RetryBackoffService<Http<Client>>, N>,
    tokio_handle: tokio::runtime::Handle,
}

impl<N: Network> RpcProvider<N> {
    pub fn new(rpc_url: String) -> anyhow::Result<Self> {
        let retry_layer = RetryBackoffLayer::new(10, 100, 330);

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

    async fn account_range(
        &self,
        block: impl Into<BlockId>,
        start: B256,
        limit: u64,
        incomplete: bool,
    ) -> TransportResult<query::AccountRangeQueryResponse> {
        self.http_client
            .client()
            .request(
                "debug_accountRange",
                (block.into(), start, limit, true, true, incomplete),
            )
            .await
    }

    async fn storage_range_at(
        &self,
        block_hash: B256,
        tx_index: u64,
        address: Address,
        key_start: B256,
        limit: u64,
    ) -> TransportResult<query::StorageRangeQueryResponse> {
        self.http_client
            .client()
            .request(
                "debug_storageRangeAt",
                (block_hash, tx_index, address, key_start, limit),
            )
            .await
    }

    #[cfg(not(feature = "erigon-range-queries"))]
    fn get_next_storage_key(
        &self,
        block: B256,
        address: Address,
        key_start: B256,
    ) -> anyhow::Result<U256> {
        let out = self
            .tokio_handle
            .block_on(
                self.storage_range_at(block, 0, address, key_start, 1)
                    .into_future(),
            )
            .context("debug_storageRangeAt failed")?;

        let (hash, entry) = out.storage.iter().next().context("no such storage slot")?;
        // Perform simple sanity checks, as this RPC is known to be wonky.
        ensure!(
            *hash >= key_start && out.next_key.map_or(true, |next| next > *hash),
            "invalid debug_storageRangeAt response"
        );
        let key = entry.key.context("preimage storage key is missing")?;

        Ok(key.0.into())
    }

    #[cfg(feature = "erigon-range-queries")]
    fn get_next_storage_key(
        &self,
        block: B256,
        address: Address,
        key_start: B256,
    ) -> anyhow::Result<U256> {
        let mut min_found: Option<(B256, B256)> = None; // (hash, key)

        let mut paging_start = B256::ZERO;
        const PAGE_SIZE: u64 = 125_000; // will result in responses < 25 MB
        let mut page_count = 1;

        log::warn!(
            "Querying all storage for address {} to find the next slot...(This might take a while)",
            address,
        );
        loop {
            let out = self
                .tokio_handle
                .block_on(
                    self.storage_range_at(block, 0, address, paging_start, PAGE_SIZE)
                        .into_future(),
                )
                .context("debug_storageRangeAt failed")?;

            for (hash, entry) in out.storage {
                let Some(alloy::serde::JsonStorageKey(key)) = entry.key else {
                    anyhow::bail!("preimage storage key is missing");
                };

                if hash >= key_start {
                    match min_found {
                        None => min_found = Some((hash, key)),
                        Some((min_hash, _)) => {
                            if hash < min_hash {
                                min_found = Some((hash, key));
                            }
                        }
                    }
                }
            }

            match out.next_key {
                Some(next_key) => {
                    let next_key_u256: U256 = next_key.into();
                    debug!(
                        "Finished querying storage range {} / {}",
                        page_count,
                        U256::MAX.div_ceil(next_key_u256 / U256::from(page_count))
                    );
                    paging_start = next_key;
                    page_count += 1;
                }
                None => break,
            }
        }

        Ok(min_found.context("no such storage key")?.1.into())
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

    fn get_next_account(&mut self, query: &NextAccountQuery) -> anyhow::Result<Address> {
        debug!("Querying RPC for next account: {:?}", query);

        let out = self
            .tokio_handle
            .block_on(
                self.account_range(query.block_no, query.start, 1, true)
                    .into_future(),
            )
            .context("debug_accountRange failed")?;
        let entry = out.accounts.values().next().context("no such account")?;
        // Perform simple sanity checks, as this RPC is known to be wonky.
        ensure!(
            entry.key >= query.start,
            "invalid debug_accountRange response"
        );

        entry.address.context("preimage address is missing")
    }

    fn get_next_slot(&mut self, query: &NextSlotQuery) -> anyhow::Result<U256> {
        debug!("Querying RPC for next storage key: {:?}", query);

        // debug_storageRangeAt returns the storage at the given block height and transaction index.
        // For this to be consistent with eth_getProof, we need to query the state after all
        // transactions have been processed, i.e. at transaction index 0 of the next block.
        let block_no = query.block_no + 1;

        // debug_storageRangeAt only accepts the block hash, not the number, so we need to query it.
        let block = self
            .tokio_handle
            .block_on(self.http_client.get_block_by_number(block_no.into(), false))
            .context("eth_getBlockByNumber failed")?
            .with_context(|| format!("block {} not found", block_no))?;
        let block_hash = block.header().hash();

        self.get_next_storage_key(block_hash, query.address, query.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::network::Ethereum;
    use alloy::primitives::address;
    use tokio::task::spawn_blocking;

    #[tokio::test]
    #[ignore = "Requires RPC node and credentials"]
    async fn get_next_slot() -> anyhow::Result<()> {
        let rpc_url = std::env::var("ETH_RPC_URL").expect("ETH_RPC_URL not set");

        let mut provider = RpcProvider::<Ethereum>::new(rpc_url)?;

        let latest = provider.http_client.get_block_number().await?;
        spawn_blocking(move || {
            provider.get_next_slot(&NextSlotQuery {
                block_no: latest - 1,
                address: address!("0xdAC17F958D2ee523a2206206994597C13D831ec7"),
                start: B256::ZERO,
            })
        })
        .await??;

        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires RPC node and credentials"]
    async fn get_next_account() -> anyhow::Result<()> {
        let rpc_url = std::env::var("ETH_RPC_URL").expect("ETH_RPC_URL not set");

        let mut provider = RpcProvider::<Ethereum>::new(rpc_url)?;

        let latest = provider.http_client.get_block_number().await?;
        spawn_blocking(move || {
            provider.get_next_account(&NextAccountQuery {
                block_no: latest,
                start: B256::ZERO,
            })
        })
        .await??;

        Ok(())
    }
}
