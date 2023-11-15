// Copyright 2023 RISC Zero, Inc.
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

use std::{fmt::Debug, time::Instant};

use anyhow::{Context, Result};
use bonsai_sdk::alpha as bonsai_sdk;
use clap::Parser;
use ethers_core::types::Transaction as EthersTransaction;
use log::{error, info};
use risc0_zkvm::{
    serde::to_vec, ExecutorEnv, ExecutorImpl, FileSegmentRef, MemoryImage, Program, Receipt,
};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use zeth_guests::{
    ETH_BLOCK_ELF, ETH_BLOCK_ID, ETH_BLOCK_PATH, OP_BLOCK_ELF, OP_BLOCK_ID, OP_BLOCK_PATH,
};
use zeth_lib::{
    builder::{BlockBuilderStrategy, EthereumStrategy, OptimismStrategy},
    consts::{ChainSpec, Network, ETH_MAINNET_CHAIN_SPEC, OP_MAINNET_CHAIN_SPEC},
    host::{preflight::Preflight, verify::Verifier},
    input::Input,
};
use zeth_primitives::BlockHash;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long, require_equals = true)]
    /// URL of the chain RPC node.
    rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata]
    cache: Option<String>,

    #[clap(
        short,
        long,
        require_equals = true,
        value_enum,
        default_value = "ethereum"
    )]
    /// Network name.
    network: Network,

    #[clap(short, long, require_equals = true)]
    /// Block number to validate.
    block_no: u64,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "20")]
    /// Runs the verification inside the zkvm executor locally. Accepts a custom maximum
    /// segment cycle count as a power of 2. [default: 20]
    local_exec: Option<u32>,

    #[clap(short, long, default_value_t = false)]
    /// Whether to submit the proving workload to Bonsai.
    submit_to_bonsai: bool,

    #[clap(short, long, require_equals = true)]
    /// Bonsai Session UUID to use for receipt verification.
    verify_bonsai_receipt_uuid: Option<String>,

    #[clap(short, long, default_value_t = false)]
    /// Whether to profile the zkVM execution
    profile: bool,
}

fn cache_file_path(cache_path: &String, network: &String, block_no: u64, ext: &str) -> String {
    format!("{}/{}/{}.{}", cache_path, network, block_no, ext)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    match args.network {
        Network::Ethereum => {
            run::<EthereumStrategy>(
                args,
                ETH_MAINNET_CHAIN_SPEC.clone(),
                ETH_BLOCK_ELF,
                ETH_BLOCK_ID,
                ETH_BLOCK_PATH,
            )
            .await
        }
        Network::Optimism => {
            run::<OptimismStrategy>(
                args,
                OP_MAINNET_CHAIN_SPEC.clone(),
                OP_BLOCK_ELF,
                OP_BLOCK_ID,
                OP_BLOCK_PATH,
            )
            .await
        }
    }
}

async fn run<N: BlockBuilderStrategy>(
    args: Args,
    chain_spec: ChainSpec,
    guest_elf: &[u8],
    guest_id: [u32; risc0_zkvm::sha::DIGEST_WORDS],
    guest_path: &str,
) -> Result<()>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    // Fetch all of the initial data
    let rpc_cache = args
        .cache
        .as_ref()
        .map(|dir| cache_file_path(dir, &args.network.to_string(), args.block_no, "json.gz"));

    let init_spec = chain_spec.clone();
    let preflight_result = tokio::task::spawn_blocking(move || {
        N::run_preflight(init_spec, rpc_cache, args.rpc_url, args.block_no)
    })
    .await?;
    let preflight_data = preflight_result.context("preflight failed")?;

    // Create the guest input from [Init]
    let input: Input<N::TxEssence> = preflight_data.clone().into();

    // Verify that the transactions run correctly
    info!("Running from memory ...");
    let (header, state_trie) =
        N::build_from(&chain_spec, input.clone()).context("Error while building block")?;

    info!("Verifying final state using provider data ...");
    preflight_data.verify_block(&header, &state_trie)?;

    info!("Final block hash derived successfully. {}", header.hash());

    // Run in the executor (if requested)
    if let Some(segment_limit_po2) = args.local_exec {
        info!(
            "Running in executor with segment_limit_po2 = {:?}",
            segment_limit_po2
        );

        let input = to_vec(&input).expect("Could not serialize input!");
        info!(
            "Input size: {} words ( {} MB )",
            input.len(),
            input.len() * 4 / 1_000_000
        );

        let mut profiler = risc0_zkvm::Profiler::new(guest_path, guest_elf).unwrap();

        info!("Running the executor...");
        let start_time = Instant::now();
        let session = {
            let mut builder = ExecutorEnv::builder();
            builder
                .session_limit(None)
                .segment_limit_po2(segment_limit_po2)
                .write_slice(&input);

            if args.profile {
                builder.trace_callback(profiler.make_trace_callback());
            }

            let env = builder.build().unwrap();
            let mut exec = ExecutorImpl::from_elf(env, guest_elf).unwrap();

            let segment_dir = tempdir().unwrap();

            exec.run_with_callback(|segment| {
                Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
            })
            .unwrap()
        };
        info!(
            "Generated {:?} segments; elapsed time: {:?}",
            session.segments.len(),
            start_time.elapsed()
        );

        if args.profile {
            profiler.finalize();

            let sys_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            tokio::fs::write(
                format!("profile_{}.pb", sys_time.as_secs()),
                &profiler.encode_to_vec(),
            )
            .await
            .expect("Failed to write profiling output");
        }

        info!(
            "Executor ran in (roughly) {} cycles",
            session.segments.len() * (1 << segment_limit_po2)
        );

        let expected_hash = preflight_data.block.hash();
        let found_hash: BlockHash = session.journal.decode().unwrap();

        if found_hash == expected_hash {
            info!("Block hash (from executor): {}", found_hash);
        } else {
            error!(
                "Final block hash mismatch (from executor) {} (expected {})",
                found_hash, expected_hash,
            );
        }
    }

    let mut bonsai_session_uuid = args.verify_bonsai_receipt_uuid;

    // Run in Bonsai (if requested)
    if bonsai_session_uuid.is_none() && args.submit_to_bonsai {
        info!("Creating Bonsai client");
        let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
            .expect("Could not create Bonsai client");

        // create the memoryImg, upload it and return the imageId
        info!("Uploading memory image");
        let img_id = {
            let program = Program::load_elf(guest_elf, risc0_zkvm::GUEST_MAX_MEM as u32)
                .expect("Could not load ELF");
            let image = MemoryImage::new(&program, risc0_zkvm::PAGE_SIZE as u32)
                .expect("Could not create memory image");
            let image_id = hex::encode(image.compute_id());
            let image = bincode::serialize(&image).expect("Failed to serialize memory img");

            client
                .upload_img(&image_id, image)
                .expect("Could not upload ELF");
            image_id
        };

        // Prepare input data and upload it.
        info!("Uploading inputs");
        let input_data = to_vec(&input).unwrap();
        let input_data = bytemuck::cast_slice(&input_data).to_vec();
        let input_id = client
            .upload_input(input_data)
            .expect("Could not upload inputs");

        // Start a session running the prover
        info!("Starting session");
        let session = client
            .create_session(img_id, input_id)
            .expect("Could not create Bonsai session");

        println!("Bonsai session UUID: {}", session.uuid);
        bonsai_session_uuid = Some(session.uuid)
    }

    // Verify receipt from Bonsai (if requested)
    if let Some(session_uuid) = bonsai_session_uuid {
        let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
            .expect("Could not create Bonsai client");
        let session = bonsai_sdk::SessionId { uuid: session_uuid };

        loop {
            let res = session
                .status(&client)
                .expect("Could not fetch Bonsai status");
            if res.status == "RUNNING" {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                continue;
            }
            if res.status == "SUCCEEDED" {
                // Download the receipt, containing the output
                let receipt_url = res
                    .receipt_url
                    .expect("API error, missing receipt on completed session");

                let receipt_buf = client
                    .download(&receipt_url)
                    .expect("Could not download receipt");
                let receipt: Receipt =
                    bincode::deserialize(&receipt_buf).expect("Could not deserialize receipt");
                receipt
                    .verify(guest_id)
                    .expect("Receipt verification failed");

                let expected_hash = preflight_data.block.hash();
                let found_hash: BlockHash = receipt.journal.decode().unwrap();

                if found_hash == expected_hash {
                    info!("Block hash (from Bonsai): {}", found_hash);
                } else {
                    error!(
                        "Final block hash mismatch (from Bonsai) {} (expected {})",
                        found_hash, expected_hash,
                    );
                }
            } else {
                panic!("Workflow exited: {}", res.status);
            }

            break;
        }
    }

    Ok(())
}
