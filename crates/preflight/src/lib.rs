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

pub mod provider;
pub mod trie;

use std::path::{Path, PathBuf};

pub fn cache_file_path(cache_path: &Path, network: &str, block_no: u64, ext: &str) -> PathBuf {
    let dir = cache_path.join(network);
    std::fs::create_dir_all(&dir).expect("Could not create directory");
    dir.join(block_no.to_string()).with_extension(ext)
}
