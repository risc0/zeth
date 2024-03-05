#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);


use zeth_lib::{builder::{BlockBuilderStrategy, TaikoStrategy}, consts::TKO_MAINNET_CHAIN_SPEC, 
    input::{GuestInput, GuestOutput, TaikoSystemInfo, TaikoProverData},
    host::host::{HostArgs, taiko_run_preflight}, EthereumTxEssence
};
use zeth_lib::protocol_instance::assemble_protocol_instance;
use zeth_lib::protocol_instance::EvidenceType;
use zeth_primitives::{keccak, Address, B256};

fn main() -> GuestOutput {

    let input: GuestInput<EthereumTxEssence> = env::read();

    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, mpt_node)) => {
            info!("Verifying final state using provider data ...");
            info!("Final block hash derived successfully. {}", header.hash());
            info!("Final block hash derived successfully. {:?}", header);
            let pi = assemble_protocol_instance(&input, &header)?
                .instance_hash(req.proof_instance.clone().into());
            GuestOutput::Success((header.clone(), pi))
        }
        Err(_) => {
            warn!("Proving bad block construction!");
            GuestOutput::Failure
        }
    };
    output
}