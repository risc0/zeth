#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);

use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput},
    EthereumTxEssence
};
use zeth_lib::protocol_instance::assemble_protocol_instance;
use zeth_lib::protocol_instance::EvidenceType;

fn main() {

    let input: GuestInput<EthereumTxEssence> = env::read();
    let build_result = TaikoStrategy::build_from(&input);

    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            let pi = assemble_protocol_instance(&input, &header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Risc0);
            GuestOutput::Success((header.clone(), pi))
        }
        Err(_) => {
            GuestOutput::Failure
        }
    };

    env::commit(&output);
}