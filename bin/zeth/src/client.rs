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

use crate::chain::Witness;
use crate::cli::Cli;
use anyhow::Context;
use log::info;
use reth_chainspec::ChainSpec;
use reth_primitives::{Block, Header};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use zeth_core::db::MemoryDB;
use zeth_core::rescue::Recoverable;
use zeth_core::stateless::client::{RethStatelessClient, StatelessClient};
use zeth_core::stateless::data::StatelessClientData;
use zeth_core::stateless::driver::{RethDriver, SCEDriver};
use zeth_core::SERDE_BRIEF_CFG;
use zeth_preflight::client::{PreflightClient, RethPreflightClient};
use zeth_preflight::derive::{RPCDerivableBlock, RPCDerivableHeader};
use zeth_preflight::provider::cache_provider::cache_dir_path;
use zeth_preflight::provider::{new_provider, BlockQuery};

#[async_trait::async_trait]
pub trait ZethClient<B, H, D, R>
where
    B: RPCDerivableBlock + Send + Serialize + DeserializeOwned + 'static,
    H: RPCDerivableHeader + Send + Serialize + DeserializeOwned + 'static,
    D: Recoverable + 'static,
    R: SCEDriver<B, H> + 'static,
    Witness: From<StatelessClientData<B, H>>,
{
    type PreflightClient: PreflightClient<B, H, R>;
    type StatelessClient: StatelessClient<B, H, D, R>;

    async fn build_block(cli: &Cli, chain_spec: Arc<ChainSpec>) -> anyhow::Result<Witness> {
        let build_args = cli.build_args().clone();

        // Fetch all of the initial data
        let cache_dir = build_args
            .cache
            .as_ref()
            .map(|dir| cache_dir_path(dir, &build_args.network.to_string()));

        let preflight_chain_spec = chain_spec.clone();
        let preflight_data: StatelessClientData<B, H> = tokio::task::spawn_blocking(move || {
            <Self::PreflightClient>::preflight(
                preflight_chain_spec,
                cache_dir,
                build_args.rpc_url,
                build_args.block_number,
                build_args.block_count,
            )
        })
        .await??;
        let build_result = Witness::from(preflight_data);

        // Verify that the transactions run correctly
        info!(
            "Running from memory (Input size: {} bytes) ...",
            build_result.encoded_input.len()
        );
        let deserialized_preflight_data: StatelessClientData<B, H> =
            serde_brief::from_slice_with_config(&build_result.encoded_input, SERDE_BRIEF_CFG)
                .context("brief deserialization failed")?;
        <Self::StatelessClient>::validate(chain_spec.clone(), deserialized_preflight_data)
            .expect("Block validation failed");
        info!("Memory run successful ...");
        Ok(build_result)
    }
}

pub struct RethZethClient;

impl ZethClient<Block, Header, MemoryDB, RethDriver> for RethZethClient {
    type PreflightClient = RethPreflightClient;
    type StatelessClient = RethStatelessClient;
}

pub async fn build_journal(cli: &Cli) -> anyhow::Result<Vec<u8>> {
    let build_args = cli.build_args().clone();

    // Fetch all of the initial data
    let cache_dir = build_args
        .cache
        .as_ref()
        .map(|dir| cache_dir_path(dir, &build_args.network.to_string()));

    // Fetch the block
    let validation_tip_block = tokio::task::spawn_blocking(move || {
        let provider =
            new_provider(cache_dir, build_args.block_number, build_args.rpc_url).unwrap();

        let result = provider.borrow_mut().get_full_block(&BlockQuery {
            block_no: build_args.block_number + build_args.block_count - 1,
        });

        result
    })
    .await??;

    let total_difficulty = validation_tip_block.header.total_difficulty.unwrap();
    let journal = [
        validation_tip_block.header.hash.0.as_slice(),
        total_difficulty.to_be_bytes::<32>().as_slice(),
        build_args.block_count.to_be_bytes().as_slice(),
    ]
    .concat();

    Ok(journal)
}
