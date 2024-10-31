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
use log::{error, info};
use risc0_zkvm::{default_executor, default_prover, ProverOpts, Receipt};
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeth::cli::{Cli, Network};
use zeth::executor::build_executor_env;
use zeth::{cache_dir_path, proof_file_name};
use zeth_guests::{
    ZETH_GUESTS_RETH_ELF, ZETH_GUESTS_RETH_ID, ZETH_GUESTS_RETH_OPTIMISM_ELF,
    ZETH_GUESTS_RETH_OPTIMISM_ID,
};
use zeth_preflight::BlockBuilder;
use zeth_preflight_ethereum::RethBlockBuilder;
use zeth_preflight_optimism::OpRethBlockBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        env_logger::init();
        let cli = Cli::parse();
        let build_args = cli.build_args();

        // Prepare the cache directory
        let cache_dir = build_args
            .cache
            .as_ref()
            .map(|dir| cache_dir_path(dir, &build_args.network.to_string()));
        // select a guest program
        let (image_id, elf, expected_journal) = match build_args.network {
            Network::Ethereum => (
                ZETH_GUESTS_RETH_ID,
                ZETH_GUESTS_RETH_ELF,
                RethBlockBuilder::build_journal(
                    cache_dir.clone(),
                    build_args.rpc_url.clone(),
                    build_args.block_number,
                    build_args.block_count,
                )
                .await?,
            ),
            Network::Optimism => (
                ZETH_GUESTS_RETH_OPTIMISM_ID,
                ZETH_GUESTS_RETH_OPTIMISM_ELF,
                OpRethBlockBuilder::build_journal(
                    cache_dir.clone(),
                    build_args.rpc_url.clone(),
                    build_args.block_number,
                    build_args.block_count,
                )
                .await?,
            ),
        };

        if !cli.should_build() {
            let verify_args = cli.verify_args();
            if build_args.block_count > 1 {
                info!(
                    "Verifying receipt file {} for blocks {} - {}.",
                    verify_args.file.display(),
                    build_args.block_number,
                    build_args.block_number + build_args.block_count
                );
            } else {
                info!(
                    "Verifying receipt file {} for block {}.",
                    verify_args.file.display(),
                    build_args.block_number
                );
            }
            let mut receipt_file = File::open(&verify_args.file).await?;
            let mut receipt_data = Vec::new();
            receipt_file.read_to_end(&mut receipt_data).await?;
            let receipt = bincode::deserialize::<Receipt>(&receipt_data)?;
            // Fail if the receipt is unverifiable or has a wrong journal
            let mut err = false;
            if receipt.journal.bytes != expected_journal {
                error!("Invalid journal.");
                dbg!(&receipt.journal.bytes);
                dbg!(&expected_journal);
                err = true;
            }
            if receipt.verify(image_id).is_err() {
                error!("Invalid proof.");
                err = true;
            };
            if err {
                panic!("Verification error.");
            }
            info!("Receipt verified successfully.");
            return Ok(());
        }

        // preflight the block building process
        let build_result = match build_args.network {
            Network::Ethereum => {
                RethBlockBuilder::build_block(
                    cache_dir.clone(),
                    build_args.rpc_url.clone(),
                    build_args.block_number,
                    build_args.block_count,
                )
                .await?
            }
            Network::Optimism => {
                OpRethBlockBuilder::build_block(
                    cache_dir.clone(),
                    build_args.rpc_url.clone(),
                    build_args.block_number,
                    build_args.block_count,
                )
                .await?
            }
        };

        if !cli.should_execute() {
            return Ok(());
        }

        // use the zkvm
        let exec_env = build_executor_env(&cli, &build_result.encoded_input)?;
        let computed_journal = if cli.should_prove() {
            info!("Proving ...");
            let prover_opts = if cli.snark() {
                ProverOpts::groth16()
            } else {
                ProverOpts::succinct()
            };
            let file_name = proof_file_name(
                build_result.validated_tail,
                build_result.validated_tip,
                image_id,
                &prover_opts,
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
                let prove_info = prover.prove_with_opts(exec_env, elf, &prover_opts)?;
                info!(
                    "Proof of {} total cycles ({} user cycles) computed.",
                    prove_info.stats.total_cycles, prove_info.stats.user_cycles
                );
                let mut output_file = File::create(&file_name).await?;
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
                info!("Receipt file {file_name} written.");
                prove_info.receipt
            };

            receipt.verify(image_id).expect("Failed to verify proof.");
            info!("Verified computed proof.");
            receipt.journal.bytes
        } else {
            info!("Executing ...");
            // run executor only
            let executor = default_executor();
            let session_info = executor.execute(exec_env, elf)?;
            info!("{} user cycles executed.", session_info.cycles());
            session_info.journal.bytes
        };
        // sanity check
        assert_eq!(expected_journal, computed_journal);
        Ok(())
    })
    .await?
}
