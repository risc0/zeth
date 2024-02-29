use std::time::Instant;

use zeth_lib::{builder::{BlockBuilderStrategy, TaikoStrategy}, consts::TKO_MAINNET_CHAIN_SPEC};

use super::{
    context::Context,
    error::Result,
    proof::{cache::Cache, sgx::execute_sgx},
    request::{ProofInstance, ProofRequest, ProofResponse},
    utils::cache_file_path,
};
#[cfg(feature = "powdr")]
use super::proof::succinct::execute_sp1;
#[cfg(feature = "succinct")]
use super::proof::powdr::execute_powdr;

use crate::{
    metrics::{inc_sgx_success, observe_input, observe_sgx_gen}, prover::proof::risc0::execute_risc0,
};
// use crate::rolling::prune_old_caches;

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
        let (input, sys_info) = prepare_input(ctx, req.clone()).await?;
        let elapsed = Instant::now().duration_since(start).as_millis() as i64;
        observe_input(elapsed);
        // 2. pre-build the block
        let output = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input);
        let elapsed = Instant::now().duration_since(elapsed).as_millis() as i64;
        observe_output(elapsed);

        // TODO: cherry-pick risc0 latest output
        match &output {
            Ok((header, mpt_node)) => {
                info!("Verifying final state using provider data ...");    
                info!("Final block hash derived successfully. {}", header.hash);
            }
            Err(_) => {
                warn!("Proving bad block construction!")
            }
        }
        // 3. run proof
        // prune_old_caches(&ctx.cache_path, ctx.max_caches);
        match req.proof_instance {
            ProofInstance::Sgx(instance) => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_sgx(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                observe_sgx_gen(bid, time_elapsed);
                inc_sgx_success(bid);
                Ok(ProofResponse::Sgx(resp))
            }
            #[cfg(feature = "powdr")]
            ProofInstance::Powdr => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_powdr().await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                todo!()
            }
            ProofInstance::PseZk => todo!(),
            #[cfg(feature = "succinct")]
            ProofInstance::Succinct => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_sp1(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                Ok(ProofResponse::SP1(resp))
            }
            ProofInstance::Risc0 => {
                execute_risc0(input, sys_info).await?;
                todo!()
            },
        }
    }
    .await;
    remove_cache_file(ctx).await?;
    result
}

/// prepare input data for guests
pub async fn prepare_input(
    ctx: &mut Context,
    req: ProofInstance,
) -> Result<(Input<EthereumTxEssence>, TaikoSystemInfo)> {
    // Todo(Cecilia): should contract address as args, curently hardcode
    let l1_cache = ctx.l1_cache_file.clone();
    let l2_cache = ctx.l2_cache_file.clone();
    let testnet = ctx.l2_contracts.clone();
    tokio::task::spawn_blocking(move || {
        init_taiko(
            HostArgs {
                l1_cache,
                l1_rpc: Some(l1_rpc),
                l2_cache,
                l2_rpc: Some(l2_rpc),
            },
            TKO_MAINNET_CHAIN_SPEC.clone(),
            &testnet,
            block,
            graffiti,
            prover,
        )
        .expect("Init taiko failed")
    })
    .await
    .map_err(Into::<Error>::into)
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
