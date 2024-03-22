use std::{env, path::PathBuf};

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use sp1_core::{utils, SP1Prover, SP1Stdin, SP1Verifier};
use zeth_lib::input::{GuestInput, GuestOutput};

// use crate::prover::{
//     consts::*,
//     context::Context,
//     request::{ProofRequest, SP1Response, SgxResponse},
// };

const ELF: &[u8] =
    include_bytes!("../../program/elf/riscv32im-succinct-zkvm-elf");

pub async fn execute_sp1(
    input: GuestInput,
) -> Result<SP1Response, String> {
    let config = utils::BabyBearBlake3::new();

    // Write the input.
    let mut stdin = SP1Stdin::new();
    stdin.write(&input);

    // Generate the proof for the given program.
    let mut proof =
        SP1Prover::prove_with_config(ELF, stdin, config.clone()).expect("proving failed");

    // Read the output.
    let output = proof.stdout.read::<GuestOutput>();

    // Verify proof.
    SP1Verifier::verify_with_config(ELF, &proof, config).expect("verification failed");

    // Save the proof.
    let proof_dir = env::current_dir().expect("dir error");
    proof
        .save(
            proof_dir
                .as_path()
                .join(format!("proof-with-io.json"))
                .to_str()
                .unwrap(),
        )
        .expect("saving proof failed");

    println!("succesfully generated and verified proof for the program!");
    Ok(SP1Response {
        proof: serde_json::to_string(&proof).unwrap(),
        output,
    })
}


#[derive(Clone, Serialize, Deserialize)]
pub struct SP1Response {
    pub proof: String,
    pub output: GuestOutput,
}