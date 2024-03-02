use std::time::Instant;

use alloy_primitives::FixedBytes;
use tracing::{info, warn};
use zeth_lib::{builder::{BlockBuilderStrategy, TaikoStrategy}, consts::TKO_MAINNET_CHAIN_SPEC, input::Input, 
    taiko::{host::{init_taiko, HostArgs}, TaikoSystemInfo}, EthereumTxEssence
};
use zeth_lib::taiko::protocol_instance::assemble_protocol_instance;
use zeth_lib::taiko::protocol_instance::EvidenceType;
use crate::metrics::{inc_sgx_success, observe_input, observe_sgx_gen};

use super::{
    context::Context,
    error::Result,
    proof::{cache::Cache, sgx::execute_sgx},
    request::{ProofInstance, ProofRequest, ProofResponse},
    utils::cache_file_path,
};
use super::proof::succinct::execute_sp1;
use super::proof::powdr::execute_powdr;
use super::proof::risc0::execute_risc0;

pub async fn execute(
    _cache: &Cache,
    ctx: &mut Context,
    req: &ProofRequest,
) -> Result<ProofResponse> {

    ctx.update_cache_path(req.block);
    // try remove cache file anyway to avoid reorg error
    // because tokio::fs::remove_file haven't guarantee of execution. So, we need to remove
    // twice
    // > Runs the provided function on an executor dedicated to blocking operations.
    // > Tasks will be scheduled as non-mandatory, meaning they may not get executed
    // > in case of runtime shutdown.
    ctx.remove_cache_file().await?;
    let result = async {
        // 1. load input data into cache path
        let start = Instant::now();
        let (input, sys_info, pi) = prepare_input(ctx, req.clone()).await?;
        let elapsed = Instant::now().duration_since(start).as_millis() as i64;
        observe_input(elapsed);
        // 2. pre-build the block
        let output = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input.clone());

        // TODO: cherry-pick risc0 latest output
        match &output {
            Ok((header, mpt_node)) => {
                info!("Verifying final state using provider data ...");    
                info!("Final block hash derived successfully. {}", header.hash());
            }
            Err(_) => {
                warn!("Proving bad block construction!")
            }
        }
        // 3. run proof
        // prune_old_caches(&ctx.cache_path, ctx.max_caches);
        match &req.proof_instance {
            ProofInstance::Sgx => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_sgx(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                observe_sgx_gen(bid, time_elapsed);
                inc_sgx_success(bid);
                Ok(ProofResponse::Sgx(resp))
            }
            ProofInstance::Powdr => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_powdr().await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                todo!()
            }
            ProofInstance::PseZk => todo!(),
            ProofInstance::Succinct => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_sp1(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                Ok(ProofResponse::SP1(resp))
            }
            ProofInstance::Risc0(instance) => {
                execute_risc0(input, pi, sys_info, ctx, instance).await?;
                todo!()
            },
        }
    }
    .await;
    ctx.remove_cache_file().await?;
    result
}

/// prepare input data for guests
pub async fn prepare_input(
    ctx: &mut Context,
    req: ProofRequest,
) -> Result<(Input<EthereumTxEssence>, TaikoSystemInfo, FixedBytes<32>)> {
    // Todo(Cecilia): should contract address as args, curently hardcode
    let l1_cache = ctx.l1_cache_file.clone();
    let l2_cache = ctx.l2_cache_file.clone();
    let (input, sys_info) = tokio::task::spawn_blocking(move || {
        init_taiko(
            HostArgs {
                l1_cache,
                l1_rpc: Some(req.l1_rpc),
                l2_cache,
                l2_rpc: Some(req.l2_rpc),
            },
            TKO_MAINNET_CHAIN_SPEC.clone(),
            &req.l2_contracts,
            req.block,
            req.graffiti,
            req.prover,
        )
        .expect("Init taiko failed")
    })
    .await?;

    let (header, _mpt_node) = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input.clone())
    .expect("Failed to build the resulting block");

    let pi = assemble_protocol_instance(&sys_info, &header)
        .expect("Failed to assemble the protocol instance")
        .instance_hash(EvidenceType::Succinct /* TODO: diff guests diff type */);
    Ok((input, sys_info, pi))
}


#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_async_block() {
        let result = async { Result::<(), &'static str>::Err("error") };
        println!("must here");
        assert!(result.await.is_err());
    }
}
