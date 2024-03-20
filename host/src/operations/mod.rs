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

pub mod build;
pub mod rollups;
pub mod snarks;

use std::fmt::Debug;

use bonsai_sdk::alpha::responses::SnarkReceipt;
use log::{debug, error, info, warn};
use risc0_zkvm::{
    compute_image_id,
    serde::to_vec,
    sha::{Digest, Digestible},
    Assumption, ExecutorEnv, ExecutorImpl, Receipt, Segment, SegmentRef,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zeth_primitives::keccak::keccak;

use crate::{cli::Cli, load_receipt, save_receipt};

pub async fn stark2snark(
    image_id: Digest,
    stark_uuid: String,
    stark_receipt: Receipt,
) -> anyhow::Result<(String, SnarkReceipt)> {
    info!("Submitting SNARK workload");
    // Label snark output as journal digest
    let receipt_label = format!(
        "{}-{}",
        hex::encode_upper(image_id),
        hex::encode(keccak(stark_receipt.journal.bytes.digest()))
    );
    // Load cached receipt if found
    if let Ok(Some(cached_data)) = load_receipt(&receipt_label) {
        info!("Loaded locally cached receipt");
        return Ok(cached_data);
    }
    // Otherwise compute on Bonsai
    let stark_uuid = if stark_uuid.is_empty() {
        upload_receipt(&stark_receipt).await?
    } else {
        stark_uuid
    };

    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    let snark_uuid = client.create_snark(stark_uuid)?;

    let snark_receipt = loop {
        let res = snark_uuid.status(&client)?;

        if res.status == "RUNNING" {
            info!("Current status: {} - continue polling...", res.status,);
            std::thread::sleep(std::time::Duration::from_secs(15));
        } else if res.status == "SUCCEEDED" {
            break res
                .output
                .expect("Bonsai response is missing SnarkReceipt.");
        } else {
            panic!(
                "Workflow exited: {} - | err: {}",
                res.status,
                res.error_msg.unwrap_or_default()
            );
        }
    };

    let stark_psd = stark_receipt.get_claim()?.post.digest();
    let snark_psd = Digest::try_from(snark_receipt.post_state_digest.as_slice())?;

    if stark_psd != snark_psd {
        error!("SNARK/STARK Post State Digest mismatch!");
        error!("STARK: {}", hex::encode(stark_psd));
        error!("SNARK: {}", hex::encode(snark_psd));
    }

    if snark_receipt.journal != stark_receipt.journal.bytes {
        error!("SNARK/STARK Receipt Journal mismatch!");
        error!("STARK: {}", hex::encode(&stark_receipt.journal.bytes));
        error!("SNARK: {}", hex::encode(&snark_receipt.journal));
    };

    let snark_data = (snark_uuid.uuid, snark_receipt);

    save_receipt(&receipt_label, &snark_data);

    Ok(snark_data)
}

pub async fn verify_bonsai_receipt<O: Eq + Debug + DeserializeOwned>(
    image_id: Digest,
    expected_output: &O,
    uuid: String,
    max_retries: usize,
) -> anyhow::Result<(String, Receipt)> {
    info!("Tracking receipt uuid: {}", uuid);
    let session = bonsai_sdk::alpha::SessionId { uuid };

    loop {
        let mut res = None;
        for attempt in 1..=max_retries {
            let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;

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
                        "Attempt {}/{} for session status request: {:?}",
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
            let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
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

pub async fn maybe_prove<I: Serialize, O: Eq + Debug + Serialize + DeserializeOwned>(
    cli: &Cli,
    input: &I,
    elf: &[u8],
    expected_output: &O,
    assumptions: (Vec<Assumption>, Vec<String>),
) -> Option<(String, Receipt)> {
    let Cli::Prove(prove_args) = cli else {
        return None;
    };

    let (assumption_instances, assumption_uuids) = assumptions;
    let encoded_input = to_vec(input).expect("Could not serialize proving input!");

    let encoded_output =
        to_vec(expected_output).expect("Could not serialize expected proving output!");
    let computed_image_id = compute_image_id(elf).expect("Failed to compute elf image id!");

    let receipt_label = format!(
        "{}-{}",
        hex::encode(computed_image_id),
        hex::encode(keccak(bytemuck::cast_slice(&encoded_output)))
    );

    // get receipt
    let (mut receipt_uuid, receipt, cached) =
        if let Ok(Some(cached_data)) = load_receipt(&receipt_label) {
            info!("Loaded locally cached receipt");
            (cached_data.0, cached_data.1, true)
        } else if prove_args.submit_to_bonsai {
            // query bonsai service until it works
            loop {
                if let Ok(remote_proof) = prove_bonsai(
                    encoded_input.clone(),
                    elf,
                    expected_output,
                    assumption_uuids.clone(),
                )
                .await
                {
                    break (remote_proof.0, remote_proof.1, false);
                }
            }
        } else {
            // run prover
            (
                Default::default(),
                prove_locally(
                    prove_args.run_args.execution_po2,
                    encoded_input,
                    elf,
                    assumption_instances,
                    prove_args.run_args.profile,
                    &cli.execution_tag(),
                ),
                false,
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

    // upload receipt to bonsai
    if prove_args.submit_to_bonsai && receipt_uuid.is_empty() {
        info!("Uploading cached receipt without UUID to Bonsai.");
        receipt_uuid = upload_receipt(&receipt)
            .await
            .expect("Failed to upload cached receipt to Bonsai");
    }

    let result = (receipt_uuid, receipt);

    // save receipt
    if !cached {
        save_receipt(&receipt_label, &result);
    }

    // return result
    Some(result)
}

pub async fn upload_receipt(receipt: &Receipt) -> anyhow::Result<String> {
    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    Ok(client.upload_receipt(bincode::serialize(receipt)?)?)
}

pub async fn prove_bonsai<O: Eq + Debug + DeserializeOwned>(
    encoded_input: Vec<u32>,
    elf: &[u8],
    expected_output: &O,
    assumption_uuids: Vec<String>,
) -> anyhow::Result<(String, Receipt)> {
    info!("Proving on Bonsai");
    // Compute the image_id, then upload the ELF with the image_id as its key.
    let image_id = risc0_zkvm::compute_image_id(elf)?;
    let encoded_image_id = hex::encode(image_id);
    // Prepare input data
    let input_data = bytemuck::cast_slice(&encoded_input).to_vec();

    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    client.upload_img(&encoded_image_id, elf.to_vec())?;
    // upload input
    let input_id = client.upload_input(input_data.clone())?;

    let session = client.create_session(
        encoded_image_id.clone(),
        input_id.clone(),
        assumption_uuids.clone(),
    )?;

    verify_bonsai_receipt(image_id, expected_output, session.uuid.clone(), 8).await
}

/// Prove the given ELF locally with the given input and assumptions. The segments are
/// stored in a temporary directory, to allow for proofs larger than the available memory.
pub fn prove_locally(
    segment_limit_po2: u32,
    encoded_input: Vec<u32>,
    elf: &[u8],
    assumptions: Vec<Assumption>,
    profile: bool,
    profile_reference: &String,
) -> Receipt {
    debug!("Proving with segment_limit_po2 = {:?}", segment_limit_po2);
    debug!(
        "Input size: {} words ( {} MB )",
        encoded_input.len(),
        encoded_input.len() * 4 / 1_000_000
    );

    info!("Running the prover...");
    let session = {
        let mut env_builder = ExecutorEnv::builder();
        env_builder
            .session_limit(None)
            .segment_limit_po2(segment_limit_po2)
            .write_slice(&encoded_input);

        if profile {
            info!("Profiling enabled.");
            env_builder.enable_profiler(format!("profile_{}.pb", profile_reference));
        }

        for assumption in assumptions {
            env_builder.add_assumption(assumption);
        }

        let env = env_builder.build().unwrap();
        let mut exec = ExecutorImpl::from_elf(env, elf).unwrap();
        exec.run().unwrap()
    };
    session.prove().unwrap()
}

const NULL_SEGMENT_REF: NullSegmentRef = NullSegmentRef {};
#[derive(Serialize, Deserialize)]
struct NullSegmentRef {}

impl SegmentRef for NullSegmentRef {
    fn resolve(&self) -> anyhow::Result<Segment> {
        unimplemented!()
    }
}

/// Execute the guest code with the given input and verify the output.
pub fn execute<T: Serialize, O: Eq + Debug + DeserializeOwned>(
    input: &T,
    segment_limit_po2: u32,
    profile: bool,
    elf: &[u8],
    expected_output: &O,
    profile_reference: &String,
) {
    debug!(
        "Running in executor with segment_limit_po2 = {:?}",
        segment_limit_po2
    );

    let input = to_vec(input).expect("Could not serialize input!");
    debug!(
        "Input size: {} words ( {} MB )",
        input.len(),
        input.len() * 4 / 1_000_000
    );

    info!("Running the executor...");
    let session = {
        let mut env_builder = ExecutorEnv::builder();
        env_builder
            .session_limit(None)
            .segment_limit_po2(segment_limit_po2)
            .write_slice(&input);

        if profile {
            info!("Profiling enabled.");
            env_builder.enable_profiler(format!("profile_{}.pb", profile_reference));
        }

        let env = env_builder.build().unwrap();
        let mut exec = ExecutorImpl::from_elf(env, elf).unwrap();

        exec.run_with_callback(|_| Ok(Box::new(NULL_SEGMENT_REF)))
            .unwrap()
    };
    println!(
        "Executor ran in (roughly) {} cycles",
        session.segments.len() * (1 << segment_limit_po2)
    );
    // verify output
    let journal = session.journal.unwrap();
    let output_guest: O = journal.decode().expect("Could not decode journal");
    if expected_output == &output_guest {
        info!("Executor succeeded");
    } else {
        error!(
            "Output mismatch! Executor: {:?}, expected: {:?}",
            output_guest, expected_output,
        );
    }
}
