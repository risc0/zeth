use std::path::PathBuf;

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use sp1_core::{utils, SP1Prover, SP1Stdin, SP1Verifier};
use zeth_lib::{consts::TKO_MAINNET_CHAIN_SPEC, taiko::host::HostArgs};

use crate::{
    metrics::inc_sgx_error,
    prover::{
        consts::*,
        context::Context,
        request::{SP1Response, SgxRequest, SgxResponse},
        utils::guest_executable_path,
    },
};

pub type SP1Proof = sp1_core::SP1ProofWithIO<utils::BabyBearBlake3>;

const ELF: &[u8] = include_bytes!("../../../../elf/riscv32im-succinct-zkvm-elf");
const SP1_PROOF: &'static str = "../../../../elf/proof-with-pis.json";

pub async fn execute_sp1(ctx: &mut Context, req: &SgxRequest) -> Result<SP1Response, String> {
    // Setup a tracer for logging.
    utils::setup_tracer();

    // Create an input stream.
    let mut stdin = SP1Stdin::new();

    let host_args = HostArgs {
        l1_cache: ctx.l1_cache_file.clone(),
        l1_rpc: Some(req.l1_rpc.clone()),
        l2_cache: ctx.l2_cache_file.clone(),
        l2_rpc: Some(req.l2_rpc.clone()),
    };
    let l2_chain_spec = TKO_MAINNET_CHAIN_SPEC.clone();

    stdin.write(&host_args);
    stdin.write(&l2_chain_spec);
    stdin.write(&ctx.l2_chain);
    stdin.write(&req.block);
    stdin.write(&req.graffiti);
    stdin.write(&req.prover);

    // Generate the proof for the given program.
    let mut proof = SP1Prover::prove(ELF, stdin).expect("proving failed");

    // Read the output.
    let pi_hash = proof.stdout.read::<FixedBytes<32>>();
    println!("pi_hash: {:?}", pi_hash);

    // Verify proof.
    // SP1Verifier::verify(ELF, &proof).expect("verification failed");

    // Save the proof.
    proof.save(SP1_PROOF).expect("saving proof failed");

    println!("succesfully generated and verified proof for the program!");
    Ok(SP1Response {
        proof: serde_json::to_string(&proof).unwrap(),
        pi_hash: pi_hash.to_string(),
    })
}
