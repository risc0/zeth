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

use std::fmt::Debug;

use anyhow::Context;
use ethers_core::types::Transaction as EthersTransaction;
use log::info;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    builder::BlockBuilderStrategy,
    consts::ChainSpec,
    host::{preflight::Preflight, verify::Verifier},
    input::Input,
};

use crate::{
    cache_file_path,
    cli::Cli,
    operations::{execute, maybe_prove},
};

pub async fn build_chain_blocks<N: BlockBuilderStrategy>(
    cli: Cli,
    file_reference: &String,
    rpc_url: Option<String>,
    chain_spec: ChainSpec,
    guest_elf: &[u8],
) -> anyhow::Result<()>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    let core_args = cli.core_args().clone();
    // Fetch all of the initial data
    let rpc_cache = core_args.cache.as_ref().map(|dir| {
        cache_file_path(
            dir,
            &core_args.network.to_string(),
            core_args.block_number,
            "json.gz",
        )
    });

    let init_spec = chain_spec.clone();
    let preflight_result = tokio::task::spawn_blocking(move || {
        N::run_preflight(init_spec, rpc_cache, rpc_url, core_args.block_number)
    })
    .await?;
    let preflight_data = preflight_result.context("preflight failed")?;

    // Create the guest input from [Init]
    let input: Input<N::TxEssence> = preflight_data
        .clone()
        .try_into()
        .context("invalid preflight data")?;

    // Verify that the transactions run correctly
    info!("Running from memory ...");
    let (header, state_trie) =
        N::build_from(&chain_spec, input.clone()).context("Error while building block")?;

    info!("Verifying final state using provider data ...");
    preflight_data.verify_block(&header, &state_trie)?;

    info!("Final block hash derived successfully. {}", header.hash());

    match &cli {
        Cli::Build(..) => {}
        Cli::Run(run_args) => {
            execute(
                &input,
                run_args.exec_args.local_exec,
                run_args.exec_args.profile,
                guest_elf,
                &preflight_data.header.hash(),
                file_reference,
            );
        }
        Cli::Prove(..) => {
            maybe_prove(
                &cli,
                &input,
                guest_elf,
                &preflight_data.header.hash(),
                vec![],
                file_reference,
                None,
            );
        }
        Cli::Verify(..) => {
            unimplemented!()
        }
        Cli::OpInfo(..) => {
            unreachable!()
        }
    }

    // let mut bonsai_session_uuid = args.verify_receipt_bonsai_uuid;

    // Run in Bonsai (if requested)
    // if bonsai_session_uuid.is_none() && args.submit_to_bonsai {
    //     info!("Creating Bonsai client");
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //
    //     // create the memoryImg, upload it and return the imageId
    //     info!("Uploading memory image");
    //     let img_id = {
    //         let program = Program::load_elf(guest_elf, risc0_zkvm::GUEST_MAX_MEM as u32)
    //             .expect("Could not load ELF");
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
    //     let input_data = to_vec(&input).unwrap();
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
    //                 .verify(guest_id)
    //                 .expect("Receipt verification failed");
    //
    //             let expected_hash = preflight_data.header.hash();
    //             let found_hash: BlockHash = receipt.journal.decode().unwrap();
    //
    //             if found_hash == expected_hash {
    //                 info!("Block hash (from Bonsai): {}", found_hash);
    //             } else {
    //                 error!(
    //                     "Final block hash mismatch (from Bonsai) {} (expected {})",
    //                     found_hash, expected_hash,
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
    // }

    Ok(())
}
