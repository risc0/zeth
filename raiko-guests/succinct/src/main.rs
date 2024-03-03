#![no_main]
sp1_zkvm::entrypoint!(main);

use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy}, consts::TKO_MAINNET_CHAIN_SPEC, input::Input, protocol_instance::{assemble_protocol_instance, EvidenceType}, EthereumTxEssence
};

pub fn main() {
    let input = sp1_zkvm::io::read::<Input<EthereumTxEssence>>();

    let (header, _mpt_node) = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input.clone())
        .expect("Failed to build the resulting block");

    let pi = assemble_protocol_instance(&input, &header)
        .expect("Failed to assemble the protocol instance");
    let pi_hash = pi.instance_hash(EvidenceType::Succinct);

    sp1_zkvm::io::write(&pi_hash);
}
