use std::time::Instant;

use zeth_lib::taiko::block_builder::TaikoStrategyBundle;

use super::{
    context::Context,
    error::Result,
    prepare_input::prepare_input,
    proof::{cache::Cache, sgx::execute_sgx},
    request::{ProofRequest, ProofResponse},
    utils::cache_file_path,
};
use crate::metrics::{inc_sgx_success, observe_input, observe_sgx_gen};
// use crate::rolling::prune_old_caches;

pub async fn execute(
    _cache: &Cache,
    ctx: &mut Context,
    req: &ProofRequest,
) -> Result<ProofResponse> {
    let (l1_cache_file, l2_cache_file) = match req {
        ProofRequest::Sgx(req) => {
            let l1_cache_file = cache_file_path(&ctx.cache_path, req.block, true);
            let l2_cache_file = cache_file_path(&ctx.cache_path, req.block, false);
            (l1_cache_file, l2_cache_file)
        }
        ProofRequest::PseZk(_) => todo!(),
    };
    // set cache file path to context
    ctx.l1_cache_file = Some(l1_cache_file);
    ctx.l2_cache_file = Some(l2_cache_file);
    // try remove cache file anyway to avoid reorg error
    // because tokio::fs::remove_file haven't guarantee of execution. So, we need to remove
    // twice
    // > Runs the provided function on an executor dedicated to blocking operations.
    // > Tasks will be scheduled as non-mandatory, meaning they may not get executed
    // > in case of runtime shutdown.
    remove_cache_file(ctx).await?;
    let result = async {
        // 1. load input data into cache path
        let start = Instant::now();
        let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
        let elapsed = Instant::now().duration_since(start).as_millis() as i64;
        observe_input(elapsed);
        // 2. run proof
        // prune_old_caches(&ctx.cache_path, ctx.max_caches);
        match req {
            ProofRequest::Sgx(req) => {
                let start = Instant::now();
                let bid = req.block;
                let resp = execute_sgx(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                observe_sgx_gen(bid, time_elapsed);
                inc_sgx_success(bid);
                Ok(ProofResponse::Sgx(resp))
            }
            ProofRequest::PseZk(_) => todo!(),
        }
    }
    .await;
    remove_cache_file(ctx).await?;
    result
}

async fn remove_cache_file(ctx: &Context) -> Result<()> {
    for file in [
        ctx.l1_cache_file.as_ref().unwrap(),
        ctx.l2_cache_file.as_ref().unwrap(),
    ] {
        tokio::fs::remove_file(file).await.or_else(|e| {
            // ignore NotFound error
            if e.kind() == ::std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })?;
    }
    Ok(())
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
