#![no_std]
#![cfg(target_os = "riscv32imac-unknown-none-elf")]


extern crate alloc;
use alloc::{collections::BTreeMap, vec};
use zeth_primitives::U256;
use zeth_lib::consts::ChainSpec;
use powdr_riscv_runtime;

#[no_mangle]
fn main() {
}