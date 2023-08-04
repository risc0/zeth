// Copyright 2023 RISC Zero, Inc.
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

use alloy_primitives::{b256, B256};
use sha3::{Digest, Keccak256};

/// Keccak hash of an empty slice.
pub const KECCAK_EMPTY: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

/// Computes the Keccak-256 hash.
/// TODO: consider switching to B256 to have consistent return types
#[inline]
pub fn keccak(data: impl AsRef<[u8]>) -> [u8; 32] {
    // TODO: remove accelerated hashing benchmark
    // std::hint::black_box(sha2::Sha256::digest(&data));
    Keccak256::digest(data).into()
}
