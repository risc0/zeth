// Copyright 2024, 2025 RISC Zero, Inc.
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
use log::{error, info, warn};
use provider::query::BlockQuery;
use reth_chainspec::NamedChain;
use std::path::PathBuf;
use tokio::task::spawn_blocking;
use zeth_core::driver::CoreDriver;
use zeth_core::rescue::Recoverable;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::data::{
    RkyvStatelessClientData, StatelessClientChainData, StatelessClientData,
};

pub mod client;
pub mod db;
pub mod driver;
pub mod provider;
pub mod trie;

#[derive(Debug, Default, Clone)]
pub struct Witness {
    pub encoded_rkyv_input: Vec<u8>,
    pub encoded_chain_input: Vec<u8>,
    pub validated_tip_hash: B256,
    pub validated_tip_number: u64,
    pub validated_tail_hash: B256,
    pub validated_tail_number: u64,
    pub chain: NamedChain,
}

impl Witness {
    pub fn driver_from<R: CoreDriver>(data: &StatelessClientData<R::Block, R::Header>) -> Self {
        let rkyv_data = RkyvStatelessClientData::from(data.clone());
        let encoded_rkyv_input = rkyv::to_bytes::<rkyv::rancor::Error>(&rkyv_data)
            .expect("rkyv serialization failed")
            .to_vec();
        let chain_data = StatelessClientChainData::<R::Block, R::Header>::from(data.clone());
        let encoded_chain_input = pot::to_vec(&chain_data).expect("pot serialization failed");
        // let encoded_input = pot::to_vec(&data).expect("serialization failed");
        let tip = R::block_header(data.blocks.last().unwrap());
        let tail = &data.parent_header;
        Self {
            encoded_rkyv_input,
            encoded_chain_input,
            validated_tip_hash: R::header_hash(tip),
            validated_tip_number: R::block_number(tip),
            validated_tail_hash: R::header_hash(tail),
            validated_tail_number: R::block_number(tail),
            chain: data.chain,
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
            // build_result.encoded_input.len()
            build_result.encoded_rkyv_input.len() + build_result.encoded_chain_input.len()
        );
        let deserialized_preflight_data: StatelessClientData<R::Block, R::Header> =
            Self::StatelessClient::data_from_parts(
                build_result.encoded_rkyv_input.as_slice(),
                build_result.encoded_chain_input.as_slice(),
            )
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
        let validation_tip_block_no = block_number + block_count - 1;
        // Fetch the block
        let (validation_tip_block, chain, client_version) = spawn_blocking(move || {
            let provider = new_provider::<N>(cache_dir, block_number, rpc_url, chain_id).unwrap();
            let mut provider_mut = provider.borrow_mut();

            let validation_tip = provider_mut
                .get_full_block(&BlockQuery {
                    block_no: validation_tip_block_no,
                })
                .unwrap();

            let client_version = provider_mut.get_client_version().unwrap();

            let chain = provider_mut.get_chain().unwrap() as u64;
            provider_mut.save().unwrap();

            (validation_tip, chain, client_version)
        })
        .await?;

        info!("Connected to provider that uses client version: {client_version}");

        let header = P::derive_header_response(validation_tip_block);

        let total_difficulty = P::total_difficulty(&header).unwrap_or_default();
        if total_difficulty.is_zero() {
            warn!("Building journal with a total chain difficulty value of zero.")
        }
        let final_difficulty = R::final_difficulty(
            validation_tip_block_no,
            total_difficulty,
            R::chain_spec(&chain.try_into().unwrap())
                .expect("Unsupported chain")
                .as_ref(),
        );

        if final_difficulty.is_zero() {
            error!("Expecting a proof with a final chain difficulty value of zero in the journal.")
        }
        let journal = [
            chain.to_be_bytes().as_slice(),
            R::header_hash(&P::derive_header(header)).0.as_slice(),
            final_difficulty.to_be_bytes::<32>().as_slice(),
            block_count.to_be_bytes().as_slice(),
        ]
        .concat();

        info!("Final chain difficulty: {}", final_difficulty);
        Ok(journal)
    }
}
