//! Prepare Input for guest
use std::fmt::Debug;

use zeth_lib::{
    block_builder::NetworkStrategyBundle,
    consts::{get_taiko_chain_spec, ETH_MAINNET_CHAIN_SPEC},
    host::{
        provider::file_provider::cache_file_path,
        taiko::{get_taiko_initial_data, TaikoExtra},
        Init,
    },
    EthereumTxEssence,
};

use super::{
    context::Context,
    error::Result,
    request::{ProofRequest, PseZkRequest, SgxRequest},
};

/// prepare input data for guests
pub async fn prepare_input<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    ctx: &mut Context,
    req: &ProofRequest,
) -> Result<(Init<N::TxEssence>, TaikoExtra)>
where
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    match req {
        ProofRequest::Sgx(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            graffiti,
        }) => {
            let l2_block = *block;

            let l2_spec = get_taiko_chain_spec(&ctx.l2_chain);
            let l2_rpc = l2_rpc.to_owned();

            let l1_spec = ETH_MAINNET_CHAIN_SPEC.clone();
            let l1_rpc = l1_rpc.to_owned();
            let prover = prover.to_owned();
            let graffiti = *graffiti;
            // run sync task in blocking mode
            let l1_cache_path = ctx.l1_cache_file.as_ref().unwrap().to_owned();
            let l2_cache_path = ctx.l2_cache_file.as_ref().unwrap().to_owned();
            tokio::task::spawn_blocking(move || {
                get_taiko_initial_data::<N>(
                    Some(l1_cache_path.into_os_string().into_string().unwrap()),
                    l1_spec,
                    Some(l1_rpc),
                    prover,
                    Some(l2_cache_path.into_os_string().into_string().unwrap()),
                    l2_spec,
                    Some(l2_rpc),
                    l2_block,
                    graffiti,
                )
            })
            .await?
            .map_err(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}
