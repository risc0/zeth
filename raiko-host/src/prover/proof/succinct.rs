use std::path::PathBuf;

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use sp1_core::{utils, SP1Prover, SP1Stdin, SP1Verifier};
use zeth_lib::{consts::TKO_TESTNET_CHAIN_SPEC};
use zeth_lib::EthereumTxEssence;
use zeth_lib::input::GuestInput;

use crate::prover::{
    consts::*,
    context::Context,
    request::{ProofRequest, SP1Response, SgxResponse},
    utils::guest_executable_path,
};
use zeth_lib::input::GuestOutput;
use std::env;

pub type SP1Proof = sp1_core::SP1ProofWithIO<utils::BabyBearBlake3>;

const ELF: &[u8] = include_bytes!("../../../../raiko-guests/succinct/elf/riscv32im-succinct-zkvm-elf");

pub async fn execute_sp1(
    input: GuestInput<EthereumTxEssence>,
    output: GuestOutput,
    ctx: &mut Context,
    req: &ProofRequest,
) -> Result<SP1Response, String> {
    let mut stdin = SP1Stdin::new();

    stdin.write(&input);

    // Generate the proof for the given program.
    let mut proof = SP1Prover::prove(ELF, stdin).expect("proving failed");

    // Read the output.
    let output = proof.stdout.read::<GuestOutput>();

    // Verify proof.
    SP1Verifier::verify(ELF, &proof).expect("verification failed");

    // Save the proof.
    let proof_dir = env::current_dir().expect("dir error");
    proof.save(proof_dir.as_path().join(format!("proof-with-pis.json")).to_str().unwrap()).expect("saving proof failed");

    println!("succesfully generated and verified proof for the program!");
    Ok(SP1Response {
        proof: serde_json::to_string(&proof).unwrap(),
        output,
    })
}
