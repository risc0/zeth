//! Prepare Input for guest

use zeth_lib::{
    consts::TKO_MAINNET_CHAIN_SPEC,
    input::Input,
    taiko::{
        host::{init_taiko, HostArgs},
        TaikoSystemInfo,
    },
    EthereumTxEssence,
};

use super::{
    context::Context,
    error::{Error, Result},
    request::{ProofRequest, PseZkRequest, SgxRequest},
};

/// prepare input data for guests
pub async fn prepare_input(
    ctx: &mut Context,
    req: ProofRequest,
) -> Result<(Input<EthereumTxEssence>, TaikoSystemInfo)> {
    match req {
        ProofRequest::Sgx(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            graffiti,
        }) => {
            // Todo(Cecilia): should contract address as args, curently hardcode
            let l1_cache = ctx.l1_cache_file.clone();
            let l2_cache = ctx.l2_cache_file.clone();
            let testnet = ctx.l2_chain.clone();
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
        },
        ProofRequest::Powdr(_) => todo!(),
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
        ProofRequest::Succinct(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            graffiti,
        }) => {
            // Todo(Cecilia): should contract address as args, curently hardcode
            let l1_cache = ctx.l1_cache_file.clone();
            let l2_cache = ctx.l2_cache_file.clone();
            let testnet = ctx.l2_chain.clone();
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
        },
    }
}
