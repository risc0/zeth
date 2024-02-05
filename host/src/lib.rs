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

use std::{fs, path::Path};

use risc0_zkvm::{is_dev_mode, Receipt};
use tracing::debug;

pub mod cli;
pub mod operations;

pub fn load_receipt(file_name: &String) -> anyhow::Result<Option<(String, Receipt)>> {
    if is_dev_mode() {
        // Nothing to load
        return Ok(None);
    }

    let receipt_serialized = match fs::read(zkp_cache_path(file_name)) {
        Ok(receipt_serialized) => receipt_serialized,
        Err(err) => {
            debug!("Could not load cached receipt with label: {}", &file_name);
            debug!("{:?}", err);
            return Ok(None);
        }
    };

    Ok(Some(bincode::deserialize(&receipt_serialized)?))
}

pub fn save_receipt(receipt_label: &String, receipt_data: &(String, Receipt)) {
    if !is_dev_mode() {
        fs::write(
            zkp_cache_path(receipt_label),
            bincode::serialize(receipt_data).expect("Failed to serialize receipt!"),
        )
        .expect("Failed to save receipt output file.");
    }
}

fn zkp_cache_path(receipt_label: &String) -> String {
    Path::new("cache_zkp")
        .join(format!("{}.zkp", receipt_label))
        .to_str()
        .unwrap()
        .to_string()
}
