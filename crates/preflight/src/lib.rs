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
use crate::driver::PreflightDriver;
use crate::provider::new_provider;
use alloy::network::Network;
use alloy::primitives::B256;
use anyhow::Context;
use log::info;
use provider::query::BlockQuery;
use std::path::PathBuf;
use tokio::task::spawn_blocking;
use zeth_core::driver::CoreDriver;
use zeth_core::rescue::Recoverable;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::data::StatelessClientData;

pub mod client;
pub mod db;
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
    pub fn driver_from<R: CoreDriver>(data: &StatelessClientData<R::Block, R::Header>) -> Self {
        let encoded_input = pot::to_vec(&data).expect("serialization failed");
        Self {
            encoded_input,
            validated_tip: R::header_hash(R::block_header(data.blocks.last().unwrap())),
            validated_tail: R::header_hash(&data.parent_header),
        }
    }
}

#[async_trait::async_trait]
pub trait BlockBuilder<N, D, R, P>
where
    N: Network,
    D: Recoverable + 'static,
    R: CoreDriver + Clone + 'static,
    <R as CoreDriver>::Block: Send + 'static,
    <R as CoreDriver>::Header: Send + 'static,
    P: PreflightDriver<R, N> + Clone + 'static,
{
    type PreflightClient: PreflightClient<N, R, P>;
    type StatelessClient: StatelessClient<R, D>;

    async fn build_blocks(
        chain_id: Option<u64>,
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_number: u64,
        block_count: u64,
    ) -> anyhow::Result<Witness> {
        // Fetch all of the initial data
        let preflight_data: StatelessClientData<R::Block, R::Header> = spawn_blocking(move || {
            <Self::PreflightClient>::preflight(
                chain_id,
                cache_dir,
                rpc_url,
                block_number,
                block_count,
            )
        })
        .await??;
        let build_result = Witness::driver_from::<R>(&preflight_data);

        // Verify that the transactions run correctly
        info!(
            "Running from memory (Input size: {} bytes) ...",
            build_result.encoded_input.len()
        );
        let deserialized_preflight_data: StatelessClientData<R::Block, R::Header> =
            Self::StatelessClient::deserialize_data(build_result.encoded_input.as_slice())
                .context("input deserialization failed")?;
        <Self::StatelessClient>::validate(deserialized_preflight_data)
            .expect("Block validation failed");
        info!("Memory run successful ...");
        Ok(build_result)
    }

    async fn build_journal(
        chain_id: Option<u64>,
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_number: u64,
        block_count: u64,
    ) -> anyhow::Result<Vec<u8>> {
        // Fetch the block
        let (validation_tip_block, chain) = spawn_blocking(move || {
            let provider = new_provider::<N>(cache_dir, block_number, rpc_url, chain_id).unwrap();
            let mut provider_mut = provider.borrow_mut();

            let validation_tip = provider_mut
                .get_full_block(&BlockQuery {
                    block_no: block_number + block_count - 1,
                })
                .unwrap();

            let chain = provider_mut.get_chain().unwrap() as u64;
            provider_mut.save().unwrap();

            (validation_tip, chain)
        })
        .await?;

        let header = P::derive_header_response(validation_tip_block);

        let total_difficulty = P::total_difficulty(&header).unwrap_or_default();
        let journal = [
            chain.to_be_bytes().as_slice(),
            R::header_hash(&P::derive_header(header)).0.as_slice(),
            total_difficulty.to_be_bytes::<32>().as_slice(),
            block_count.to_be_bytes().as_slice(),
        ]
        .concat();

        Ok(journal)
    }
}
