use std::str;

use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, info};

use crate::{
    metrics::inc_sgx_error,
    prover::{
        consts::*,
        context::Context,
        request::{SgxRequest, SgxResponse},
        utils::guest_executable_path,
    },
};

pub async fn execute_sgx(ctx: &mut Context, req: &SgxRequest) -> Result<SgxResponse, String> {
    let guest_path = guest_executable_path(&ctx.guest_path, SGX_PARENT_DIR);
    debug!("Guest path: {:?}", guest_path);
    let mut cmd = {
        let bin_directory = guest_path
            .parent()
            .ok_or(String::from("missing sgx executable directory"))?;
        let bin = guest_path
            .file_name()
            .ok_or(String::from("missing sgx executable"))?;
        let mut cmd = Command::new("sudo");
        cmd.current_dir(bin_directory)
            .arg("gramine-sgx")
            .arg(bin)
            .arg("one-shot");
        cmd
    };
    let output = cmd
        .arg("--blocks-data-file")
        .arg(ctx.l2_cache_file.as_ref().unwrap())
        .arg("--l1-blocks-data-file")
        .arg(ctx.l1_cache_file.as_ref().unwrap())
        .arg("--prover")
        .arg(req.prover.to_string())
        .arg("--graffiti")
        .arg(req.graffiti.to_string())
        .arg("--sgx-instance-id")
        .arg(ctx.sgx_context.instance_id.to_string())
        .arg("--l2-chain")
        .arg(&ctx.l2_chain)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    info!("Sgx execution stderr: {:?}", str::from_utf8(&output.stderr));
    info!("Sgx execution stdout: {:?}", str::from_utf8(&output.stdout));
    if !output.status.success() {
        inc_sgx_error(req.block);
        Err(output.status.to_string())
    } else {
        parse_sgx_result(output.stdout)
    }
}

fn parse_sgx_result(output: Vec<u8>) -> Result<SgxResponse, String> {
    let mut json_value: Option<Value> = None;
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;

    for line in output.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            json_value = Some(value);
            break;
        }
    }

    let extract_field = |field| {
        json_value
            .as_ref()
            .and_then(|json| json.get(field).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string()
    };

    let proof = extract_field("proof");
    let quote = extract_field("quote");

    Ok(SgxResponse { proof, quote })
}
