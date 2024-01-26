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

pub mod chains;
pub mod info;
pub mod rollups;

use std::fmt::Debug;

use bonsai_sdk::alpha as bonsai_sdk;
use log::{error, info, warn};
use risc0_zkvm::{
    default_prover, serde::to_vec, sha::Digest, Assumption, ExecutorEnv, ExecutorImpl,
    FileSegmentRef, Receipt, Session,
};
use serde::{de::DeserializeOwned, Serialize};
use tempfile::tempdir;

use crate::{cli::Cli, save_receipt};

pub fn verify_bonsai_receipt<O: Eq + Debug + DeserializeOwned>(
    image_id: Digest,
    expected_output: &O,
    uuid: String,
    client: Option<&bonsai_sdk::Client>,
    max_retries: usize,
) -> anyhow::Result<(String, Receipt)> {
    info!("Tracking receipt uuid: {}", uuid);
    let mut local_client = None;

    let client = client.unwrap_or_else(|| {
        local_client = Some(bonsai_sdk::Client::from_env(risc0_zkvm::VERSION).unwrap());
        local_client.as_ref().unwrap()
    });

    let session = bonsai_sdk::SessionId { uuid };

    loop {
        let mut res = None;
        for attempt in 1..=max_retries {
            match session.status(&client) {
                Ok(response) => {
                    res = Some(response);
                    break;
                }
                Err(err) => {
                    if attempt == max_retries {
                        anyhow::bail!(err);
                    }
                    warn!(
                        "Attempt {}/{} for session status request: {}",
                        attempt, max_retries, err
                    );
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    continue;
                }
            }
        }

        let res = res.unwrap();

        if res.status == "RUNNING" {
            info!(
                "Current status: {} - state: {} - continue polling...",
                res.status,
                res.state.unwrap_or_default()
            );
            std::thread::sleep(std::time::Duration::from_secs(15));
        } else if res.status == "SUCCEEDED" {
            // Download the receipt, containing the output
            let receipt_url = res
                .receipt_url
                .expect("API error, missing receipt on completed session");

            let receipt_buf = client.download(&receipt_url)?;
            let receipt: Receipt = bincode::deserialize(&receipt_buf)?;
            receipt
                .verify(image_id)
                .expect("Receipt verification failed");
            // verify output
            let receipt_output: O = receipt.journal.decode().unwrap();
            if expected_output == &receipt_output {
                info!("Receipt validated!");
            } else {
                error!(
                    "Output mismatch! Receipt: {:?}, expected: {:?}",
                    receipt_output, expected_output,
                );
            }
            return Ok((session.uuid, receipt));
        } else {
            panic!(
                "Workflow exited: {} - | err: {}",
                res.status,
                res.error_msg.unwrap_or_default()
            );
        }
    }
}

pub fn maybe_prove<I: Serialize, O: Eq + Debug + DeserializeOwned>(
    cli: &Cli,
    input: &I,
    elf: &[u8],
    expected_output: &O,
    assumptions: (Vec<Assumption>, Vec<String>),
    file_reference: &String,
    receipt_index: Option<&mut usize>,
) -> Option<(String, Receipt)> {
    let (assumption_instances, assumption_uuids) = assumptions;
    if let Cli::Prove(prove_args) = cli {
        let encoded_input = to_vec(input).expect("Could not serialize composition prep input!");
        let (receipt_uuid, receipt) = if prove_args.submit_to_bonsai {
            // query bonsai service
            prove_bonsai(encoded_input, elf, expected_output, assumption_uuids)
                .expect("Failed to prove on Bonsai")
        } else {
            // run prover
            (
                Default::default(),
                prove_locally(
                    prove_args.exec_args.local_exec,
                    encoded_input,
                    elf,
                    assumption_instances,
                    prove_args.exec_args.profile,
                    file_reference,
                ),
            )
        };
        // verify output
        let output_guest: O = receipt.journal.decode().unwrap();
        if expected_output == &output_guest {
            info!("Prover succeeded");
        } else {
            error!(
                "Output mismatch! Prover: {:?}, expected: {:?}",
                output_guest, expected_output,
            );
        }
        // save receipt
        save_receipt(file_reference, &receipt, receipt_index);
        // return result
        Some((receipt_uuid, receipt))
    } else {
        None
    }
}

pub fn prove_bonsai<O: Eq + Debug + DeserializeOwned>(
    encoded_input: Vec<u32>,
    elf: &[u8],
    expected_output: &O,
    assumption_uuids: Vec<String>,
) -> anyhow::Result<(String, Receipt)> {
    info!("Proving on Bonsai");
    let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)?;

    // Compute the image_id, then upload the ELF with the image_id as its key.
    let image_id = risc0_zkvm::compute_image_id(elf)?;
    let encoded_image_id = hex::encode(image_id);
    // Prepare input data
    let input_data = bytemuck::cast_slice(&encoded_input).to_vec();

    // if at first you don't succeed...
    loop {
        match client.upload_img(&encoded_image_id, elf.to_vec()) {
            Err(err) => {
                warn!("Retrying image upload: {}", err);
                std::thread::sleep(std::time::Duration::from_secs(15));
                continue;
            }
            _ => {}
        }

        // upload input
        let input_id = loop {
            match client.upload_input(input_data.clone()) {
                Ok(session) => break session,
                Err(err) => {
                    warn!("Retrying input upload: {}", err);
                    std::thread::sleep(std::time::Duration::from_secs(15));
                }
            }
        };

        // Start a session running the prover
        let session = match client.create_session(
            encoded_image_id.clone(),
            input_id.clone(),
            assumption_uuids.clone(),
        ) {
            Ok(session) => session,
            Err(err) => {
                warn!("Retrying session creation request: {}", err);
                std::thread::sleep(std::time::Duration::from_secs(15));
                continue;
            }
        };

        // Return the result
        match verify_bonsai_receipt(image_id, expected_output, session.uuid, Some(&client), 4) {
            Err(err) => {
                warn!("Retrying session creation: {}", err);
            }
            result => return result,
        }
    }
}

pub fn prove_locally(
    segment_limit_po2: u32,
    encoded_input: Vec<u32>,
    elf: &[u8],
    assumptions: Vec<Assumption>,
    profile: bool,
    file_reference: &String,
) -> Receipt {
    info!("Proving with segment_limit_po2 = {:?}", segment_limit_po2);
    info!(
        "Input size: {} words ( {} MB )",
        encoded_input.len(),
        encoded_input.len() * 4 / 1_000_000
    );

    info!("Running the prover...");
    let mut env_builder = ExecutorEnv::builder();

    env_builder
        .session_limit(None)
        .segment_limit_po2(segment_limit_po2)
        .write_slice(&encoded_input);

    if profile {
        info!("Profiling enabled.");
        env_builder.enable_profiler(format!("profile_{}.pb", file_reference));
    }

    for assumption in assumptions {
        env_builder.add_assumption(assumption);
    }

    let prover = default_prover();
    prover.prove(env_builder.build().unwrap(), elf).unwrap()
}

pub fn execute<T: serde::Serialize + ?Sized, O: Eq + Debug + DeserializeOwned>(
    input: &T,
    segment_limit_po2: u32,
    profile: bool,
    elf: &[u8],
    expected_output: &O,
    file_reference: &String,
) -> Session {
    info!(
        "Running in executor with segment_limit_po2 = {:?}",
        segment_limit_po2
    );

    let input = to_vec(input).expect("Could not serialize input!");
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

        if profile {
            info!("Profiling enabled.");
            builder.enable_profiler(format!("profile_{}.pb", file_reference));
        }

        let env = builder.build().unwrap();
        let mut exec = ExecutorImpl::from_elf(env, elf).unwrap();

        let segment_dir = tempdir().unwrap();

        exec.run_with_callback(|segment| {
            Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
        })
        .unwrap()
    };
    // verify output
    let output_guest: O = session.journal.clone().unwrap().decode().unwrap();
    if expected_output == &output_guest {
        info!("Executor succeeded");
    } else {
        error!(
            "Output mismatch! Executor: {:?}, expected: {:?}",
            output_guest, expected_output,
        );
    }
    // report performance
    println!(
        "Generated {:?} segments; elapsed time: {:?}",
        session.segments.len(),
        start_time.elapsed()
    );
    println!(
        "Executor ran in (roughly) {} cycles",
        session.segments.len() * (1 << segment_limit_po2)
    );
    // return result
    session
}
