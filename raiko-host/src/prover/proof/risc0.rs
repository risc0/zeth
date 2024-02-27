use std::path::PathBuf;

use alloy_primitives::FixedBytes;
use serde::{Deserialize, Serialize};
use zeth_lib::{consts::TKO_MAINNET_CHAIN_SPEC, taiko::host::HostArgs};

use crate::prover::{
    consts::*,
    context::Context,
    request::{SgxRequest, SgxResponse},
    utils::guest_executable_path,
};

pub async fn execute_risc0(ctx: &mut Context, req: &SgxRequest) -> Result<SgxResponse, String> {
    
    todo!()
}