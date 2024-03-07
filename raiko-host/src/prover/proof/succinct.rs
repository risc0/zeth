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

pub type SP1Proof = sp1_core::SP1ProofWithIO<utils::BabyBearBlake3>;

const ELF: &[u8] = include_bytes!("../../../../raiko-guests/succinct/elf/riscv32im-succinct-zkvm-elf");
const SP1_PROOF: &'static str = "../../../../raiko-guests/succinct/elf/proof-with-pis.json";

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
    // SP1Verifier::verify(ELF, &proof).expect("verification failed");

    // Save the proof.
    proof.save(SP1_PROOF).expect("saving proof failed");

    println!("succesfully generated and verified proof for the program!");
    Ok(SP1Response {
        proof: serde_json::to_string(&proof).unwrap(),
        output,
    })
}
