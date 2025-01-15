// Copyright 2025 RISC Zero, Inc.
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

use std::hash::{BuildHasher, Hasher};

#[derive(Clone, Default)]
pub struct NoMapHasher;

impl BuildHasher for NoMapHasher {
    type Hasher = NoHasher;

    fn build_hasher(&self) -> Self::Hasher {
        NoHasher::default()
    }
}

#[derive(Default)]
pub struct NoHasher([u8; 8]);

impl Hasher for NoHasher {
    fn finish(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    fn write(&mut self, bytes: &[u8]) {
        let l = std::cmp::min(8, bytes.len());
        self.0[..l].copy_from_slice(&bytes[..l]);
    }
}
