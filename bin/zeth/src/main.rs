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

use clap::Parser;
use log::info;
use reth_chainspec::MAINNET;
use risc0_zkvm::{default_executor, default_prover, ProverOpts};
use zeth::cli::{Cli, Network};
use zeth::client::{RethZethClient, ZethClient};
use zeth::executor::build_executor_env;
use zeth_guests::{RETH_ELF, RETH_ID};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let build_args = cli.build_args();

    // select a gues
    let (_image_id, elf) = match build_args.network {
        Network::Ethereum => (RETH_ID, RETH_ELF),
        Network::Optimism => todo!(),
    };

    if !cli.should_build() {
        // todo: verify receipt
        return Ok(());
    }

    // preflight the block building process
    let input = match build_args.network {
        Network::Ethereum => {
            RethZethClient::build_block(&cli, build_args.eth_rpc_url.clone(), MAINNET.clone())
                .await?
        }
        Network::Optimism => todo!(),
    };

    if !cli.should_execute() {
        return Ok(());
    }

    // use the zkvm
    let exec_env = build_executor_env(&cli, &input)?;
    if cli.should_prove() {
        info!("Proving ...");
        // run prover
        let prover = default_prover();
        let _prove_info = prover.prove_with_opts(exec_env, elf, &ProverOpts::succinct())?;
    } else {
        info!("Executing ...");
        // run executor only
        let executor = default_executor();
        let _session_info = executor.execute(exec_env, elf)?;
    }

    // todo: bonsai

    // // Create/verify Groth16 SNARK
    // if cli.snark() {
    //     let Some((stark_uuid, stark_receipt)) = stark else {
    //         panic!("No STARK data to snarkify!");
    //     };
    //
    //     if !cli.submit_to_bonsai() {
    //         panic!("Bonsai submission flag required to create a SNARK!");
    //     }
    //
    //     let image_id = Digest::from(image_id);
    //     let (snark_uuid, snark_receipt) = stark2snark(image_id, stark_uuid, stark_receipt).await?;
    //
    //     info!("Validating SNARK uuid: {}", snark_uuid);
    //
    //     verify_groth16_snark(&cli, image_id, snark_receipt).await?;
    // }

    Ok(())
}
