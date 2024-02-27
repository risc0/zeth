extern crate core;

use anyhow::Result;
use clap::Parser;
use log::info;
use risc0_zkvm::sha::Digest;
use risc0_driver::{
    cli::{Cli, Network},
    operations::{/* build, rollups, */ snarks::verify_groth16_snark, stark2snark},
};
use risc0_guest::*;
use zeth_lib::{
    builder::{EthereumStrategy, TaikoStrategy},
    consts::{ETH_MAINNET_CHAIN_SPEC, TKO_MAINNET_CHAIN_SPEC},
};

#[tokio::main]
async fn main() -> Result<()> {
    Ok(())
}