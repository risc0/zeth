// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    fs,
    path::{Path, PathBuf},
};

use risc0_zkvm::Receipt;

pub mod cli;

pub fn cache_file_path(cache_path: &Path, network: &str, block_no: u64, ext: &str) -> PathBuf {
    cache_path
        .join(network)
        .join(block_no.to_string())
        .with_extension(ext)
}

pub fn save_receipt(file_reference: &String, receipt: &Receipt, index: Option<&mut usize>) {
    let receipt_serialized = bincode::serialize(receipt).expect("Failed to serialize receipt!");
    let path = if let Some(number) = index {
        *number += 1;
        format!("receipt_{}-{}.zkp", file_reference, *number - 1)
    } else {
        format!("receipt_{}.zkp", file_reference)
    };
    fs::write(path, receipt_serialized).expect("Failed to save receipt output file.");
}
