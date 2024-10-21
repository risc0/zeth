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
use alloy::primitives::U256;
use anyhow::Context;
use log::{info, warn};
use reth_chainspec::ChainSpec;
use reth_primitives::{Block, Header};
use reth_revm::InMemoryDB;
use risc0_zkvm::Receipt;
use std::sync::Arc;
use zeth_core::stateless::client::{RethStatelessClient, StatelessClient};
use zeth_preflight::client::{PreflightClient, RethPreflightClient};
use zeth_preflight::derive::{RPCDerivableBlock, RPCDerivableHeader};
use zeth_preflight::provider::cache_provider::cache_file_path;

#[async_trait::async_trait]
pub trait ZethClient<B, H, D>
where
    B: RPCDerivableBlock + Send + 'static,
    H: RPCDerivableHeader + Send + 'static,
{
    type PreflightClient: PreflightClient<B, H>;
    type StatelessClient: StatelessClient<B, H, D>;

    async fn build_block<'a>(
        cli: &'a Cli,
        rpc_url: Option<String>,
        chain_spec: Arc<ChainSpec>,
        _guest_elf: &'a [u8],
    ) -> anyhow::Result<Option<(String, Receipt)>> {
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

        // Verify that the transactions run correctly
        info!("Running from memory ...");
        <Self::StatelessClient>::validate_block(
            chain_spec.clone(),
            preflight_data,
            U256::ZERO, // todo: load this up
        )
        .expect("Block validation failed");

        // let output = N::build_from(chain_spec, input.clone()).context("Error while building block")?;
        //
        // match &output {
        //     BlockBuildOutput::SUCCESS {
        //         hash, head, state, ..
        //     } => {
        //         info!("Verifying final state using provider data ...");
        //         preflight_data.verify_block(head, state)?;
        //
        //         info!("Final block hash derived successfully. {}", hash);
        //     }
        //     BlockBuildOutput::FAILURE { .. } => {
        //         warn!("Proving bad block construction!")
        //     }
        // }
        //
        // let compressed_output = output.with_state_hashed();
        // let result = match cli {
        //     Cli::Build(..) => None,
        //     Cli::Run(run_args) => {
        //         execute(
        //             &input,
        //             run_args.execution_po2,
        //             run_args.profile,
        //             guest_elf,
        //             &compressed_output,
        //             &cli.execution_tag(),
        //         );
        //         None
        //     }
        //     Cli::Prove(..) => {
        //         maybe_prove(
        //             cli,
        //             &input,
        //             guest_elf,
        //             &compressed_output,
        //             Default::default(),
        //         )
        //         .await
        //     }
        //     Cli::Verify(verify_args) => Some(
        //         verify_bonsai_receipt(
        //             compute_image_id(guest_elf)?,
        //             &compressed_output,
        //             verify_args.bonsai_receipt_uuid.clone(),
        //             4,
        //         )
        //         .await?,
        //     ),
        // };

        Ok(None)
    }
}

pub struct RethZethClient;

impl ZethClient<Block, Header, InMemoryDB> for RethZethClient {
    type PreflightClient = RethPreflightClient;
    type StatelessClient = RethStatelessClient;
}
