use std::time::Instant;

use tracing::{info, warn};
use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::{assemble_protocol_instance, EvidenceType},
    taiko_utils::HeaderHasher,
};
use zeth_primitives::Address;

use super::{
    context::Context,
    error::Result,
    proof::{
        cache::Cache, powdr::execute_powdr, risc0::execute_risc0, sgx::execute_sgx,
        succinct::execute_sp1,
    },
    request::{ProofRequest, ProofResponse, ProofType},
};
use crate::{
    host::host::taiko_run_preflight,
    metrics::{inc_sgx_success, observe_input, observe_sgx_gen},
};

pub async fn execute(
    _cache: &Cache,
    ctx: &mut Context,
    req: &ProofRequest,
) -> Result<ProofResponse> {
    // ctx.update_cache_path(req.block);
    // try remove cache file anyway to avoid reorg error
    // because tokio::fs::remove_file haven't guarantee of execution. So, we need to remove
    // twice
    // > Runs the provided function on an executor dedicated to blocking operations.
    // > Tasks will be scheduled as non-mandatory, meaning they may not get executed
    // > in case of runtime shutdown.
    // ctx.remove_cache_file().await?;
    let result = async {
        println!("- {:?}", req);
        // 1. load input data into cache path
        let start = Instant::now();
        let input = prepare_input(ctx, req.clone()).await?;
        let elapsed = Instant::now().duration_since(start).as_millis() as i64;
        observe_input(elapsed);
        // 2. pre-build the block
        let build_result = TaikoStrategy::build_from(&input);
        // TODO: cherry-pick risc0 latest output
        let output = match &build_result {
            Ok((header, mpt_node)) => {
                info!("Verifying final state using provider data ...");
                info!("Final block hash derived successfully. {}", header.hash());
                info!("Final block header derived successfully. {:?}", header);
                let pi = assemble_protocol_instance(&input, &header)?
                    .instance_hash(req.proof_type.clone().into());

                // Make sure the blockhash from the node matches the one from the builder
                assert_eq!(
                    header.hash().0,
                    input.block_hash.to_fixed_bytes(),
                    "block hash unexpected"
                );
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
        let start = Instant::now();
        let proof = match &req.proof_type {
            ProofType::Sgx => {
                let bid = req.block_number;
                let resp = execute_sgx(ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                observe_sgx_gen(bid, time_elapsed);
                inc_sgx_success(bid);
                ProofResponse::Sgx(resp)
            }
            ProofType::Powdr => {
                let bid = req.block_number;
                let resp = execute_powdr().await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                todo!()
            }
            ProofType::PseZk => todo!(),
            ProofType::Succinct => {
                let bid = req.block_number;
                let resp = execute_sp1(input, output, ctx, req).await?;
                let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
                ProofResponse::SP1(resp)
            }
            ProofType::Risc0(instance) => {
                let resp = execute_risc0(input, output, ctx, instance).await?;
                ProofResponse::Risc0(resp)
            }
            ProofType::Native => {
                // Make sure the binary serialization of the input works
                let encoded: Vec<u8> = bincode::serialize(&input).unwrap();
                let input = bincode::deserialize(&encoded[..]).unwrap();
                // Build the block
                let build_result = TaikoStrategy::build_from(&input);
                match &build_result {
                    Ok((header, mpt_node)) => {
                        // Make sure the blockhash from the node matches the one from the builder
                        assert_eq!(
                            header.hash().0,
                            input.block_hash.to_fixed_bytes(),
                            "block hash unexpected"
                        );
                        ProofResponse::Native(output)
                    }
                    Err(_) => {
                        warn!("Proving bad block construction!");
                        ProofResponse::Native(GuestOutput::Failure)
                    }
                }
            }
        };
        let time_elapsed = Instant::now().duration_since(start);
        println!(
            "Proof generated in {}.{} seconds",
            time_elapsed.as_secs(),
            time_elapsed.subsec_millis()
        );
        Ok(proof)
    }
    .await;
    ctx.remove_cache_file().await?;
    result
}

/// prepare input data for guests
pub async fn prepare_input(
    ctx: &mut Context,
    req: ProofRequest,
) -> Result<GuestInput> {
    // Todo(Cecilia): should contract address as args, curently hardcode
    let l1_cache = ctx.l1_cache_file.clone();
    let l2_cache = ctx.l2_cache_file.clone();
    tokio::task::spawn_blocking(move || {
        taiko_run_preflight(
            Some(req.l1_rpc),
            Some(req.l2_rpc),
            req.block_number,
            &req.chain,
            TaikoProverData {
                graffiti: req.graffiti,
                prover: req.prover,
            },
            Some(req.beacon_rpc),
        )
        .expect("Failed to fetch required data for block")
    })
    .await
    .map_err(Into::<super::error::Error>::into)
}

impl From<ProofType> for EvidenceType {
    fn from(value: ProofType) -> Self {
        match value {
            ProofType::Succinct => EvidenceType::Succinct,
            ProofType::PseZk => EvidenceType::PseZk,
            ProofType::Powdr => EvidenceType::Powdr,
            ProofType::Sgx => EvidenceType::Sgx {
                new_pubkey: Address::default(),
            },
            ProofType::Risc0(_) => EvidenceType::Risc0,
            ProofType::Native => EvidenceType::Native,
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
