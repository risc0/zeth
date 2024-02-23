use std::{
    path::{Path, PathBuf},
    str,
};

use powdr::{
    pipeline::test_util::verify_pipeline,
    riscv::{compile_rust, CoProcessors},
    GoldilocksField, Pipeline,
};
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, info};

use crate::prover::error::{Error, Result};

pub async fn execute_powdr() -> Result<(), Error> {
    println!("Compiling Rust...");
    let (asm_file_path, asm_contents) = compile_rust(
        "/raiko-guest/Cargo.toml",
        Path::new("/tmp/test"),
        true,
        &CoProcessors::base().with_poseidon(),
        // use bootloader
        false,
    )
    .ok_or_else(|| vec!["could not compile rust".to_string()])
    .unwrap();
    println!("Compilation done.");
    println!("Creating pipeline...");
    let pipeline: Pipeline<GoldilocksField> = Pipeline::default()
        .from_asm_string(asm_contents, Some(PathBuf::from(asm_file_path)))
        .with_prover_inputs(vec![]);
    println!("Pipeline done.");
    println!("Verifying pipeline...");
    verify_pipeline(pipeline);
    println!("Verification done.");
    Ok(())
}
// phoebe@cecilia-gz:~/projects/zeth$
//  cargo +nightly build --release -Z build-std=core,alloc --target
// riscv32imac-unknown-none-elf --lib --manifest-path ./raiko-guest/Cargo.toml
