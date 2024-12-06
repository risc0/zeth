#![no_main]

use risc0_zkvm::guest::env;
use zeth_lib::{
    builder::{BlockBuilderStrategy, LineaStrategy},
    consts::LINEA_MAINNET_CHAIN_SPEC,
};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    // Read the input previous block and transaction data
    let input = env::read();
    // Build the resulting block
    let mut output = LineaStrategy::build_from(&LINEA_MAINNET_CHAIN_SPEC, input)
        .expect("Failed to build the resulting block");
    // Abridge successful construction results
    if let Some(replaced_state) = output.replace_state_with_hash() {
        // Leak memory, save cycles
        core::mem::forget(replaced_state);
    }
    // Output the construction result
    env::commit(&output);
    // Leak memory, save cycles
    core::mem::forget(output);
}
