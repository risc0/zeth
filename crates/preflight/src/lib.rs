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

use crate::client::PreflightClient;
use crate::derive::{RPCDerivableBlock, RPCDerivableHeader};
use crate::provider::{new_provider, BlockQuery};
use alloy::primitives::B256;
use anyhow::Context;
use log::info;
use reth_chainspec::ChainSpec;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use zeth_core::rescue::Recoverable;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core::stateless::driver::SCEDriver;

pub mod client;
pub mod db;
pub mod derive;
pub mod driver;
pub mod provider;
pub mod trie;

#[derive(Debug, Default, Clone)]
pub struct Witness {
    pub encoded_input: Vec<u8>,
    pub validated_tip: B256,
    pub validated_tail: B256,
}

impl Witness {
    pub fn driver_from<B: Serialize, H: Serialize, R: SCEDriver<B, H>>(
        data: &StatelessClientData<B, H>,
    ) -> Self {
        let encoded_input = pot::to_vec(&data).expect("serialization failed");
        Self {
            encoded_input,
            validated_tip: R::header_hash(R::block_header(data.blocks.last().unwrap())),
            validated_tail: R::header_hash(&data.parent_header),
        }
    }
}

#[async_trait::async_trait]
pub trait BlockBuilder<B, H, D, R>
where
    B: RPCDerivableBlock + Send + Serialize + DeserializeOwned + 'static,
    H: RPCDerivableHeader + Send + Serialize + DeserializeOwned + 'static,
    D: Recoverable + 'static,
    R: SCEDriver<B, H> + 'static,
{
    type PreflightClient: PreflightClient<B, H, R>;
    type StatelessClient: StatelessClient<B, H, D, R>;

    fn chain_spec() -> Arc<ChainSpec>;

    async fn build_block(
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_number: u64,
        block_count: u64,
    ) -> anyhow::Result<Witness> {
        // Fetch all of the initial data
        let preflight_data: StatelessClientData<B, H> = tokio::task::spawn_blocking(move || {
            <Self::PreflightClient>::preflight(
                Self::chain_spec(),
                cache_dir,
                rpc_url,
                block_number,
                block_count,
            )
        })
        .await??;
        let build_result = Witness::driver_from::<B, H, R>(&preflight_data);

        // Verify that the transactions run correctly
        info!(
            "Running from memory (Input size: {} bytes) ...",
            build_result.encoded_input.len()
        );
        let deserialized_preflight_data: StatelessClientData<B, H> =
            Self::StatelessClient::deserialize_data(build_result.encoded_input.as_slice())
                .context("input deserialization failed")?;
        <Self::StatelessClient>::validate(Self::chain_spec(), deserialized_preflight_data)
            .expect("Block validation failed");
        info!("Memory run successful ...");
        Ok(build_result)
    }

    async fn build_journal(
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_number: u64,
        block_count: u64,
    ) -> anyhow::Result<Vec<u8>> {
        // Fetch the block
        let validation_tip_block = tokio::task::spawn_blocking(move || {
            let provider = new_provider(cache_dir, block_number, rpc_url).unwrap();

            let result = provider.borrow_mut().get_full_block(&BlockQuery {
                block_no: block_number + block_count - 1,
            });

            result
        })
        .await??;

        let total_difficulty = validation_tip_block.header.total_difficulty.unwrap();
        let journal = [
            validation_tip_block.header.hash.0.as_slice(),
            total_difficulty.to_be_bytes::<32>().as_slice(),
            block_count.to_be_bytes().as_slice(),
        ]
        .concat();

        Ok(journal)
    }
}
