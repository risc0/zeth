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
use risc0_zkvm::{default_executor, default_prover, ProverOpts, Receipt};
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeth::cli::{Cli, Network};
use zeth::client::{RethZethClient, ZethClient};
use zeth::executor::build_executor_env;
use zeth::proof_file_name;
use zeth_guests::{RETH_ELF, RETH_ID};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let build_args = cli.build_args();

    // select a gues
    let (image_id, elf) = match build_args.network {
        Network::Ethereum => (RETH_ID, RETH_ELF),
        Network::Optimism => todo!(),
    };

    if !cli.should_build() {
        // todo: verify receipt
        return Ok(());
    }

    // preflight the block building process
    let build_result = match build_args.network {
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
    let exec_env = build_executor_env(&cli, &build_result.encoded_input)?;
    if cli.should_prove() {
        info!("Proving ...");
        let file_name = proof_file_name(
            build_result.validated_tail,
            build_result.validated_tip,
            image_id,
        );
        let receipt = if let Ok(true) = Path::new(&file_name).try_exists() {
            info!("Proving skipped. Receipt file {file_name} already exists.");
            let mut receipt_file = File::open(file_name).await?;
            let mut receipt_data = Vec::new();
            receipt_file.read_to_end(&mut receipt_data).await?;
            bincode::deserialize::<Receipt>(&receipt_data)?
        } else {
            info!("Computing uncached receipt. This might take some time.");
            // run prover
            let prover = default_prover();
            let prover_opts = if cli.snark() {
                ProverOpts::groth16()
            } else {
                ProverOpts::succinct()
            };
            let prove_info = prover.prove_with_opts(exec_env, elf, &prover_opts)?;
            info!(
                "Proof of {} total cycles ({} user cycles) computed.",
                prove_info.stats.total_cycles, prove_info.stats.user_cycles
            );
            let mut output_file = File::create(file_name).await?;
            // Write receipt data to file
            let receipt_bytes =
                bincode::serialize(&prove_info.receipt).expect("Could not serialize receipt.");
            output_file
                .write_all(receipt_bytes.as_slice())
                .await
                .expect("Failed to write receipt to file");
            output_file
                .flush()
                .await
                .expect("Failed to flush receipt output file data.");
            prove_info.receipt
        };

        receipt.verify(image_id).expect("Failed to verify proof.");
        info!("Verified computed proof.")
    } else {
        info!("Executing ...");
        // run executor only
        let executor = default_executor();
        let session_info = executor.execute(exec_env, elf)?;
        info!("{} user cycles executed.", session_info.cycles());
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
