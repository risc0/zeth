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

// Example usage:
//
// RUST_LOG=info ./target/release/op-derive \
// --eth-rpc-url="https://eth-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
// --op-rpc-url="https://opt-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
// --cache \
// --op-block-no=109279674 \
// --op-blocks=6

use std::path::PathBuf;

use anyhow::{Context, Result};
// use bonsai_sdk::alpha as bonsai_sdk;
use clap::Parser;
use log::{error, info};
use risc0_zkvm::{serde::to_vec, ExecutorEnv, ExecutorImpl, FileSegmentRef};
use tempfile::tempdir;
use zeth_guests::*;
use zeth_lib::{
    host::rpc_db::RpcDb,
    optimism::{config::OPTIMISM_CHAIN_SPEC, DeriveInput, DeriveMachine, DeriveOutput},
};

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, require_equals = true)]
    /// URL of the L1 RPC node.
    eth_rpc_url: Option<String>,

    #[clap(long, require_equals = true)]
    /// URL of the L2 RPC node.
    op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata/derivation")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata/derivation]
    cache: Option<PathBuf>,

    #[clap(long, require_equals = true)]
    /// L2 block number to begin from
    op_block_no: u64,

    #[clap(long, require_equals = true)]
    /// Number of L2 blocks to provably derive.
    op_blocks: u64,

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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    info!("Fetching data ...");
    let (derive_input, output) = tokio::task::spawn_blocking(move || {
        let derive_input = DeriveInput {
            db: RpcDb::new(args.eth_rpc_url, args.op_rpc_url, args.cache),
            op_head_block_no: args.op_block_no,
            op_derive_block_count: args.op_blocks,
        };
        let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
            .context("Could not create derive machine")?;
        let derive_output = derive_machine.derive().context("could not derive")?;
        let derive_input_mem = DeriveInput {
            db: derive_machine.derive_input.db.get_mem_db(),
            op_head_block_no: args.op_block_no,
            op_derive_block_count: args.op_blocks,
        };
        let out: Result<_> = Ok((derive_input_mem, derive_output));
        out
    })
    .await?
    .context("preflight failed")?;

    info!("Running from memory ...");
    {
        let output_mem = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input.clone())
            .context("Could not create derive machine")?
            .derive()
            .unwrap();
        assert_eq!(output, output_mem);
    }

    info!("In-memory test complete");
    println!("Eth tail: {} {}", output.eth_tail.0, output.eth_tail.1);
    println!("Op Head: {} {}", output.op_head.0, output.op_head.1);
    for derived_block in &output.derived_op_blocks {
        println!("Derived: {} {}", derived_block.0, derived_block.1);
    }

    // Run in the executor (if requested)
    if let Some(segment_limit_po2) = args.local_exec {
        info!(
            "Running in executor with segment_limit_po2 = {:?}",
            segment_limit_po2
        );

        let input = to_vec(&derive_input).expect("Could not serialize input!");
        info!(
            "Input size: {} words ( {} MB )",
            input.len(),
            input.len() * 4 / 1_000_000
        );

        info!("Running the executor...");
        let start_time = std::time::Instant::now();
        let session = {
            let mut builder = ExecutorEnv::builder();
            builder
                .session_limit(None)
                .segment_limit_po2(segment_limit_po2)
                .write_slice(&input);

            if args.profile {
                info!("Profiling enabled.");
                let sys_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap();

                builder.enable_profiler(format!("profile_opd_{}.pb", sys_time.as_secs()));
            }

            let env = builder.build().unwrap();
            let mut exec = ExecutorImpl::from_elf(env, OP_DERIVE_ELF).unwrap();

            let segment_dir = tempdir().unwrap();

            exec.run_with_callback(|segment| {
                Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
            })
            .unwrap()
        };
        println!(
            "Generated {:?} segments; elapsed time: {:?}",
            session.segments.len(),
            start_time.elapsed()
        );

        println!(
            "Executor ran in (roughly) {} cycles",
            session.segments.len() * (1 << segment_limit_po2)
        );

        let output_guest: DeriveOutput = session.journal.unwrap().decode().unwrap();

        if output == output_guest {
            println!("Executor succeeded");
        } else {
            error!(
                "Output mismatch! Executor: {:?}, expected: {:?}",
                output_guest, output,
            );
        }
    }

    // let mut bonsai_session_uuid = args.verify_bonsai_receipt_uuid;

    // Run in Bonsai (if requested)
    // if bonsai_session_uuid.is_none() && args.submit_to_bonsai {
    //     info!("Creating Bonsai client");
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //
    //     // create the memoryImg, upload it and return the imageId
    //     info!("Uploading memory image");
    //     let img_id = {
    //         let program = Program::load_elf(OP_DERIVE_ELF, risc0_zkvm::GUEST_MAX_MEM as
    // u32)             .expect("Could not load ELF");
    //         let image = MemoryImage::new(&program, risc0_zkvm::PAGE_SIZE as u32)
    //             .expect("Could not create memory image");
    //         let image_id = hex::encode(image.compute_id());
    //         let image = bincode::serialize(&image).expect("Failed to serialize memory
    // img");
    //
    //         client
    //             .upload_img(&image_id, image)
    //             .expect("Could not upload ELF");
    //         image_id
    //     };
    //
    //     // Prepare input data and upload it.
    //     info!("Uploading inputs");
    //     let input_data = to_vec(&derive_input).unwrap();
    //     let input_data = bytemuck::cast_slice(&input_data).to_vec();
    //     let input_id = client
    //         .upload_input(input_data)
    //         .expect("Could not upload inputs");
    //
    //     // Start a session running the prover
    //     info!("Starting session");
    //     let session = client
    //         .create_session(img_id, input_id)
    //         .expect("Could not create Bonsai session");
    //
    //     println!("Bonsai session UUID: {}", session.uuid);
    //     bonsai_session_uuid = Some(session.uuid)
    // }

    // Verify receipt from Bonsai (if requested)
    // if let Some(session_uuid) = bonsai_session_uuid {
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //     let session = bonsai_sdk::SessionId { uuid: session_uuid };
    //
    //     loop {
    //         let res = session
    //             .status(&client)
    //             .expect("Could not fetch Bonsai status");
    //         if res.status == "RUNNING" {
    //             println!(
    //                 "Current status: {} - state: {} - continue polling...",
    //                 res.status,
    //                 res.state.unwrap_or_default()
    //             );
    //             tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    //             continue;
    //         }
    //         if res.status == "SUCCEEDED" {
    //             // Download the receipt, containing the output
    //             let receipt_url = res
    //                 .receipt_url
    //                 .expect("API error, missing receipt on completed session");
    //
    //             let receipt_buf = client
    //                 .download(&receipt_url)
    //                 .expect("Could not download receipt");
    //             let receipt: Receipt =
    //                 bincode::deserialize(&receipt_buf).expect("Could not deserialize
    // receipt");             receipt
    //                 .verify(OP_DERIVE_ID)
    //                 .expect("Receipt verification failed");
    //
    //             let bonsai_output: DeriveOutput = receipt.journal.decode().unwrap();
    //
    //             if output == bonsai_output {
    //                 println!("Bonsai succeeded");
    //             } else {
    //                 error!(
    //                     "Output mismatch! Bonsai: {:?}, expected: {:?}",
    //                     bonsai_output, output,
    //                 );
    //             }
    //         } else {
    //             panic!(
    //                 "Workflow exited: {} - | err: {}",
    //                 res.status,
    //                 res.error_msg.unwrap_or_default()
    //             );
    //         }
    //
    //         break;
    //     }
    //
    //     info!("Bonsai request completed");
    // }

    Ok(())
}
