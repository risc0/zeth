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
use crate::executor::build_executor_env;
use alloy::network::Network;
use alloy::primitives::B256;
use clap::Parser;
use log::{error, info};
use risc0_zkvm::{default_executor, default_prover, is_dev_mode, ProverOpts, Receipt};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::runtime::Handle;
use zeth_core::driver::CoreDriver;
use zeth_core::keccak::keccak;
use zeth_core::rescue::Recoverable;
use zeth_preflight::driver::PreflightDriver;
use zeth_preflight::BlockBuilder;

pub mod cli;
pub mod executor;

pub fn run<
    B: BlockBuilder<C, N, D, R, P> + Send,
    C,
    N: Network,
    D: Recoverable + 'static,
    R: CoreDriver + Clone + 'static,
    P: PreflightDriver<R, N> + Clone + 'static,
>(
    elf: &[u8],
    image_id: [u32; 8],
    network_name: &str,
) -> anyhow::Result<()>
where
    <R as CoreDriver>::Block: Send + 'static,
    <R as CoreDriver>::Header: Send + 'static,
{
    env_logger::init();
    let cli = Cli::parse();
    let build_args = cli.build_args();

    // Prepare the cache directory
    let cache_dir = build_args
        .cache
        .as_ref()
        .map(|dir| cache_dir_path(dir, network_name));
    // select a guest program
    let expected_journal = Handle::current().block_on(B::build_journal(
        cache_dir.clone(),
        build_args.rpc_url.clone(),
        build_args.block_number,
        build_args.block_count,
    ))?;
    info!("Journal prepared.");

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
        let mut receipt_file = File::open(&verify_args.file)?;
        let mut receipt_data = Vec::new();
        receipt_file.read_to_end(&mut receipt_data)?;
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
    let build_result = Handle::current().block_on(B::build_block(
        cache_dir.clone(),
        build_args.rpc_url.clone(),
        build_args.block_number,
        build_args.block_count,
    ))?;

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
            let mut receipt_file = File::open(file_name)?;
            let mut receipt_data = Vec::new();
            receipt_file.read_to_end(&mut receipt_data)?;
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
            let mut output_file = File::create(&file_name)?;
            // Write receipt data to file
            let receipt_bytes =
                bincode::serialize(&prove_info.receipt).expect("Could not serialize receipt.");
            output_file
                .write_all(receipt_bytes.as_slice())
                .expect("Failed to write receipt to file");
            output_file
                .flush()
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
}

pub fn proof_file_name(
    first_block_hash: B256,
    last_block_hash: B256,
    image_id: [u32; 8],
    prover_opts: &ProverOpts,
) -> String {
    let prover_opts = bincode::serialize(prover_opts).unwrap();
    let version = risc0_zkvm::get_version().unwrap();
    let suffix = if is_dev_mode() { "fake" } else { "zkp" };
    let data = [
        bytemuck::cast::<_, [u8; 32]>(image_id).as_slice(),
        first_block_hash.as_slice(),
        last_block_hash.as_slice(),
        prover_opts.as_slice(),
    ]
    .concat();
    let file_name = B256::from(keccak(data));
    format!("risc0-{version}-{file_name}.{suffix}")
}

pub fn cache_dir_path(cache_path: &Path, network: &str) -> PathBuf {
    let dir = cache_path.join(network);
    std::fs::create_dir_all(&dir).expect("Could not create directory");
    dir
}
