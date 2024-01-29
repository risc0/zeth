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

use std::fs;

use risc0_zkvm::{is_dev_mode, Receipt};
use tracing::debug;

pub mod cli;
pub mod operations;

pub fn load_receipt(
    file_name: &String,
    require_uuid: bool,
) -> anyhow::Result<Option<(String, Receipt)>> {
    if is_dev_mode() {
        // Nothing to load
        return Ok(None);
    }

    let receipt_serialized = match fs::read(receipt_extension(file_name)) {
        Ok(receipt_serialized) => receipt_serialized,
        Err(err) => {
            debug!("Could not load cached receipt with label: {}", &file_name);
            debug!("{:?}", err);
            return Ok(None);
        }
    };

    let result: (String, Receipt) = bincode::deserialize(&receipt_serialized)?;
    if result.0.is_empty() && require_uuid {
        // saved local receipt while uuid is needed
        return Ok(None);
    }

    Ok(Some(result))
}

pub fn save_receipt(receipt_label: &String, receipt: &Receipt) {
    if is_dev_mode() {
        // nothing to save
        return;
    }
    let receipt_serialized = bincode::serialize(receipt).expect("Failed to serialize receipt!");

    fs::write(receipt_extension(receipt_label), receipt_serialized)
        .expect("Failed to save receipt output file.");
}

fn receipt_extension(receipt_label: &String) -> String {
    format!("{}.zkp", receipt_label)
}
