use std::time::Instant;

use zeth_lib::taiko::block_builder::TaikoStrategyBundle;

use super::{
    context::Context,
    error::Result,
    prepare_input::prepare_input,
    proof::{cache::Cache, sgx::execute_sgx},
    request::{ProofRequest, ProofResponse},
};
use crate::metrics::{inc_sgx_success, observe_input, observe_sgx_gen};
// use crate::rolling::prune_old_caches;

pub async fn execute(_cache: &Cache, ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
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
            let bid = req.block.clone();
            let resp = execute_sgx(ctx, req).await?;
            let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
            observe_sgx_gen(bid, time_elapsed);
            inc_sgx_success(bid);
            Ok(ProofResponse::Sgx(resp))
        }
        ProofRequest::PseZk(_) => todo!(),
    }
}
