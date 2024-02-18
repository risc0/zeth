#![no_main] 
// when not target_os = "zkvm", no main is compiled
#![cfg(target_os = "zkvm")]

use std::{collections::BTreeMap, vec};

fn main() {
}