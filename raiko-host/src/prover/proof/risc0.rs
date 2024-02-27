use std::path::PathBuf;

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use sp1_core::{utils, SP1Prover, SP1Stdin, SP1Verifier};
use zeth_lib::{consts::TKO_MAINNET_CHAIN_SPEC, taiko::host::HostArgs};

use crate::prover::{
    consts::*,
    context::Context,
    request::{SP1Response, SgxRequest, SgxResponse},
    utils::guest_executable_path,
};

pub async fn execute_risc0(ctx: &mut Context, req: &SgxRequest) -> Result<SP1Response, String> {
    
    todo!()
}