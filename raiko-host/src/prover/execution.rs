use std::time::Instant;

use alloy_primitives::FixedBytes;
use ethers_core::types::H160;
use tracing::{info, warn};
use zeth_lib::{builder::{BlockBuilderStrategy, TaikoStrategy}, consts::TKO_MAINNET_CHAIN_SPEC, input::Input, 
    taiko::{host::{init_taiko, HostArgs}, 
    protocol_instance::{self, ProtocolInstance}, GuestOutput, TaikoSystemInfo}, EthereumTxEssence
};
use zeth_lib::taiko::protocol_instance::assemble_protocol_instance;
use zeth_lib::taiko::protocol_instance::EvidenceType;
use zeth_primitives::{keccak, Address, B256};
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
        // Todo(Cecilia): should contract address as args, curently hardcode
        let l1_cache = ctx.l1_cache_file.clone();
        let l2_cache = ctx.l2_cache_file.clone();
        let req_ = req.clone();
        let (input, sys_info) = tokio::task::spawn_blocking(move || {
            init_taiko(
                HostArgs {
                    l1_cache,
                    l1_rpc: Some(req_.l1_rpc),
                    l2_cache,
                    l2_rpc: Some(req_.l2_rpc),
                },
                TKO_MAINNET_CHAIN_SPEC.clone(),
                &req_.l2_contracts,
                req_.block,
                req_.graffiti,
                req_.prover,
            )
            .expect("Init taiko failed")
        })
        .await?;
        // 2. pre-build the block
        let build_result = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input.clone());
        // TODO: cherry-pick risc0 latest output
        let output = match &build_result {
            Ok((header, mpt_node)) => {
                info!("Verifying final state using provider data ...");    
                info!("Final block hash derived successfully. {}", header.hash());
                let pi = assemble_protocol_instance(&sys_info, &header)?
                    .instance_hash(req.proof_instance.clone().into());
                GuestOutput::Success((header.clone(), pi))
            }
            Err(_) => {
                warn!("Proving bad block construction!");
                GuestOutput::Failure
            }
        };
        let elapsed = Instant::now().duration_since(start).as_millis() as i64;
        observe_input(elapsed);
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
                execute_risc0(input, output, sys_info, ctx, instance).await?;
                todo!()
            },
            ProofInstance::Native => {
                Ok(ProofResponse::Native(output))
            },
        }
    }
    .await;
    ctx.remove_cache_file().await?;
    result
}



impl From<ProofInstance> for EvidenceType {
    fn from(value: ProofInstance) -> Self {
        match value {
            ProofInstance::Succinct => EvidenceType::Succinct,
            ProofInstance::PseZk => EvidenceType::PseZk,
            ProofInstance::Powdr => EvidenceType::Powdr,
            ProofInstance::Sgx => EvidenceType::Sgx{
                new_pubkey: Address::default()
            },
            ProofInstance::Risc0(_) => EvidenceType::Risc0,
            ProofInstance::Native => EvidenceType::Native,
        }
    }
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
