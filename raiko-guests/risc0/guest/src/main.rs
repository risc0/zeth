#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);


use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy}, 
    consts::TKO_MAINNET_CHAIN_SPEC, 
    input::{GuestInput, GuestOutput, TaikoSystemInfo, TaikoProverData},
    host::host::{HostArgs, taiko_run_preflight}, EthereumTxEssence
};
use zeth_lib::protocol_instance::assemble_protocol_instance;
use zeth_lib::protocol_instance::EvidenceType;
use zeth_primitives::{keccak, Address, B256};

fn main(){

    let input: GuestInput<EthereumTxEssence> = env::read();
    let build_result = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input.clone());

    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, mpt_node)) => {
            let pi = assemble_protocol_instance(&input, &header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Risc0);
            GuestOutput::Success((header.clone(), pi))
        }
        Err(_) => {
            GuestOutput::Failure
        }
    };
    env::write(&output);
}