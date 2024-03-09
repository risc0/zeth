

#![no_main]
sp1_zkvm::entrypoint!(main);

use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput},
    EthereumTxEssence
};
use zeth_lib::protocol_instance::assemble_protocol_instance;
use zeth_lib::protocol_instance::EvidenceType;

pub fn main() {

    let input = sp1_zkvm::io::read::<GuestInput<EthereumTxEssence>>();
    let build_result = TaikoStrategy::build_from(&input);

    let output = match &build_result {
        Ok((header, mpt_node)) => {
            let pi = assemble_protocol_instance(&input, &header)
                .expect("Failed to assemble protocol instance")
                .instance_hash(EvidenceType::Succinct);
            GuestOutput::Success((header.clone(), pi))
        }
        Err(_) => {
            GuestOutput::Failure
        }
    };

    sp1_zkvm::io::write(&output);
}
