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

use crate::cli::Cli;
use anyhow::Context;
use log::{info, warn};
use reth_chainspec::ChainSpec;
use reth_primitives::{Block, Header};
use reth_revm::InMemoryDB;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use zeth_core::stateless::client::{RethStatelessClient, StatelessClient};
use zeth_core::stateless::data::StatelessClientData;
use zeth_preflight::client::{PreflightClient, RethPreflightClient};
use zeth_preflight::derive::{RPCDerivableBlock, RPCDerivableHeader};
use zeth_preflight::provider::cache_provider::cache_file_path;

#[async_trait::async_trait]
pub trait ZethClient<B, H, D>
where
    B: RPCDerivableBlock + Send + Serialize + DeserializeOwned + 'static,
    H: RPCDerivableHeader + Send + Serialize + DeserializeOwned + 'static,
{
    type PreflightClient: PreflightClient<B, H>;
    type StatelessClient: StatelessClient<B, H, D>;

    async fn build_block(
        cli: &Cli,
        rpc_url: Option<String>,
        chain_spec: Arc<ChainSpec>,
    ) -> anyhow::Result<Vec<u8>> {
        let build_args = cli.build_args().clone();
        if build_args.block_count > 1 {
            warn!("Building multiple blocks is not supported. Only the first block will be built.");
        }

        // Fetch all of the initial data
        let rpc_cache = build_args.cache.as_ref().map(|dir| {
            cache_file_path(
                dir,
                &build_args.network.to_string(),
                build_args.block_number,
                "json.gz",
            )
        });

        let preflight_chain_spec = chain_spec.clone();
        let preflight_result = tokio::task::spawn_blocking(move || {
            <Self::PreflightClient>::preflight_with_rpc(
                preflight_chain_spec,
                rpc_cache,
                rpc_url,
                build_args.block_number,
            )
        })
        .await?;
        let preflight_data = preflight_result.context("preflight failed")?;
        let brief_config = serde_brief::Config {
            use_indices: true,
            error_on_excess_data: false,
            max_size: None,
        };
        let briefed_input = serde_brief::to_vec_with_config(&preflight_data, brief_config)
            .context("brief serialization failed")?;
        let _: StatelessClientData<B, H> =
            serde_brief::from_slice_with_config(&briefed_input, brief_config)
                .context("brief deserialization failed")?;

        // Verify that the transactions run correctly
        info!("Running from memory (Input size: {} bytes) ...", briefed_input.len());
        <Self::StatelessClient>::validate_block(chain_spec.clone(), preflight_data)
            .expect("Block validation failed");
        info!("Memory run successful ...");
        Ok(briefed_input)
    }
}

pub struct RethZethClient;

impl ZethClient<Block, Header, InMemoryDB> for RethZethClient {
    type PreflightClient = RethPreflightClient;
    type StatelessClient = RethStatelessClient;
}
