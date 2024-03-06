#![no_main]
sp1_zkvm::entrypoint!(main);

use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy}, protocol_instance::{assemble_protocol_instance, EvidenceType}, EthereumTxEssence
};
use zeth_lib::protocol_instance::assemble_protocol_instance;
use zeth_lib::protocol_instance::EvidenceType;
use zeth_primitives::{keccak, Address, B256};

pub fn main() {

    let (header, _mpt_node) = TaikoStrategy::build_from(&input)
        .expect("Failed to build the resulting block");

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
    sp1_zkvm::io::write(&output);
}
