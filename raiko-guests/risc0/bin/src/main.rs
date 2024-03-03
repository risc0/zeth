#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);


use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy}, consts::{ChainSpec, TKO_MAINNET_CHAIN_SPEC},
    input::{self, Input},
    taiko::{
        host::{HostArgs},
        protocol_instance::{assemble_protocol_instance, EvidenceType}, TaikoSystemInfo,
    }
};
use zeth_primitives::{Address, B256};

fn main() {

    let input: Input<zeth_lib::EthereumTxEssence> = env::read();
    let sys_info: TaikoSystemInfo = env::read();

    let (header, _mpt_node) = TaikoStrategy::build_from(&TKO_MAINNET_CHAIN_SPEC.clone(), input)
        .expect("Failed to build the resulting block");

    let pi = assemble_protocol_instance(&sys_info, &header)
        .expect("Failed to assemble the protocol instance");
    let pi_hash = pi.instance_hash(EvidenceType::Succinct);

}