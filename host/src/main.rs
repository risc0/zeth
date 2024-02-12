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
use log::info;
use risc0_zkvm::sha::Digest;
use zeth::{
    cli::{Cli, Network},
    operations::{build, rollups, snarks::verify_groth16_snark, stark2snark},
};
use zeth_guests::*;
use zeth_lib::{
    builder::{EthereumStrategy, OptimismStrategy},
    consts::{ETH_MAINNET_CHAIN_SPEC, OP_MAINNET_CHAIN_SPEC},
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    info!("Using the following image ids:");
    info!("  eth-block: {}", Digest::from(ETH_BLOCK_ID));
    info!("  op-block: {}", Digest::from(OP_BLOCK_ID));
    info!("  op-derive: {}", Digest::from(OP_DERIVE_ID));
    info!("  op-compose: {}", Digest::from(OP_COMPOSE_ID));

    // execute the command
    let build_args = cli.build_args();
    let (image_id, stark) = match build_args.network {
        Network::Ethereum => {
            let rpc_url = build_args.eth_rpc_url.clone();
            (
                ETH_BLOCK_ID,
                build::build_block::<EthereumStrategy>(
                    &cli,
                    rpc_url,
                    &ETH_MAINNET_CHAIN_SPEC,
                    ETH_BLOCK_ELF,
                )
                .await?,
            )
        }
        Network::Optimism => {
            let rpc_url = build_args.op_rpc_url.clone();
            (
                OP_BLOCK_ID,
                build::build_block::<OptimismStrategy>(
                    &cli,
                    rpc_url,
                    &OP_MAINNET_CHAIN_SPEC,
                    OP_BLOCK_ELF,
                )
                .await?,
            )
        }
        Network::OptimismDerived => {
            if let Some(composition_size) = build_args.composition {
                (
                    OP_COMPOSE_ID,
                    rollups::compose_derived_rollup_blocks(&cli, composition_size).await?,
                )
            } else {
                (OP_DERIVE_ID, rollups::derive_rollup_blocks(&cli).await?)
            }
        }
    };

    // Create/verify Groth16 SNARK
    if cli.snark() {
        let Some((stark_uuid, stark_receipt)) = stark else {
            panic!("No STARK data to snarkify!");
        };

        if !cli.submit_to_bonsai() {
            panic!("Bonsai submission flag required to create a SNARK!");
        }

        let image_id = Digest::from(image_id);
        let (snark_uuid, snark_receipt) = stark2snark(image_id, stark_uuid, stark_receipt).await?;

        info!("Validating SNARK uuid: {}", snark_uuid);

        verify_groth16_snark(&cli, image_id, snark_receipt).await?;
    }

    Ok(())
}
