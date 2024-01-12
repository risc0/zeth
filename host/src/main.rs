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

extern crate core;

use anyhow::Result;
use clap::Parser;
use zeth::{
    cli::Cli,
    operations::{chains, info, rollups},
};
use zeth_guests::*;
use zeth_lib::{
    builder::{EthereumStrategy, OptimismStrategy},
    consts::{Network, ETH_MAINNET_CHAIN_SPEC, OP_MAINNET_CHAIN_SPEC},
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    // Run simple debug info command
    if let Cli::OpInfo(..) = &cli {
        return info::op_info(cli).await;
    }

    // Execute other commands
    let core_args = cli.core_args();
    let sys_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let file_reference = format!("{}_{}", sys_time.as_secs(), cli.to_string());

    match core_args.network {
        Network::Ethereum => {
            let rpc_url = core_args.eth_rpc_url.clone();
            chains::build_chain_blocks::<EthereumStrategy>(
                cli,
                &file_reference,
                rpc_url,
                ETH_MAINNET_CHAIN_SPEC.clone(),
                ETH_BLOCK_ELF,
            )
            .await
        }
        Network::Optimism => {
            let rpc_url = core_args.op_rpc_url.clone();
            chains::build_chain_blocks::<OptimismStrategy>(
                cli,
                &file_reference,
                rpc_url,
                OP_MAINNET_CHAIN_SPEC.clone(),
                OP_BLOCK_ELF,
            )
            .await
        }
        Network::OptimismDerived => {
            if let Some(composition_size) = cli.composition() {
                rollups::compose_derived_rollup_blocks(cli, composition_size, &file_reference).await
            } else {
                rollups::derive_rollup_blocks(cli, &file_reference).await
            }
        }
    }
}
