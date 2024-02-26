#![no_main]
use risc0_zkvm::guest::env;
risc0_zkvm::guest::entry!(main);


use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::{ChainSpec, TKO_MAINNET_CHAIN_SPEC},
    taiko::{
        host::{init_taiko, HostArgs},
        protocol_instance::{assemble_protocol_instance, EvidenceType},
    },
};
use zeth_primitives::{Address, B256};

fn main() {
    // let x: u32 = env::read();
    // let y: u32 = env::read();

}