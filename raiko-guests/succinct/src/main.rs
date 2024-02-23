#![no_main]
sp1_zkvm::entrypoint!(main);


use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::{ChainSpec, TKO_MAINNET_CHAIN_SPEC},
    taiko::{
        host::{init_taiko, HostArgs},
        protocol_instance::{assemble_protocol_instance, EvidenceType},
    },
};
use zeth_primitives::{Address, B256};

pub fn main() {
    let host_args = sp1_zkvm::io::read::<HostArgs>();
    let l2_chain_spec = sp1_zkvm::io::read::<ChainSpec>();
    let testnet = sp1_zkvm::io::read::<String>();
    let l2_block_no = sp1_zkvm::io::read::<u64>();
    let graffiti = sp1_zkvm::io::read::<B256>();
    let prover = sp1_zkvm::io::read::<Address>();

    let (input, sys_info) = init_taiko(
        host_args,
        l2_chain_spec,
        &testnet,
        l2_block_no,
        graffiti,
        prover,
    )
    .unwrap();

    let (header, _mpt_node) = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input)
        .expect("Failed to build the resulting block");

    let pi = assemble_protocol_instance(&sys_info, &header)
        .expect("Failed to assemble the protocol instance");
    let pi_hash = pi.instance_hash(EvidenceType::Succinct);

    sp1_zkvm::io::write(&pi_hash);
}
