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

use log::{error, info};
use risc0_zkvm::{
    default_prover, serde::to_vec, Assumption, ExecutorEnv, ExecutorImpl, FileSegmentRef, Receipt,
    Session,
};
use serde::{de::DeserializeOwned, Serialize};
use tempfile::tempdir;

use crate::{cli::Cli, save_receipt};

pub fn maybe_prove<I: Serialize, O: Eq + Debug + DeserializeOwned>(
    cli: &Cli,
    input: &I,
    elf: &[u8],
    expected_output: &O,
    assumptions: Vec<Assumption>,
    file_reference: &String,
    receipt_index: Option<&mut usize>,
) -> Option<Receipt> {
    if let Cli::Prove(prove_args) = cli {
        if prove_args.submit_to_bonsai {
            unimplemented!()
        }
        // run prover
        let receipt = prove(
            prove_args.exec_args.local_exec,
            to_vec(input).expect("Could not serialize composition prep input!"),
            elf,
            assumptions,
            prove_args.exec_args.profile,
            file_reference,
        );
        // verify output
        let output_guest: O = receipt.journal.decode().unwrap();
        if expected_output == &output_guest {
            info!("Executor succeeded");
        } else {
            error!(
                "Output mismatch! Executor: {:?}, expected: {:?}",
                output_guest, expected_output,
            );
        }
        // save receipt
        save_receipt(file_reference, &receipt, receipt_index);
        // return result
        Some(receipt)
    } else {
        None
    }
}

pub fn prove(
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
