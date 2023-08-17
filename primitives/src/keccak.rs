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

/// Represents the Keccak-256 hash of an empty byte slice.
///
/// This is a constant value and can be used as a default or placeholder
/// in various cryptographic operations.
pub const KECCAK_EMPTY: B256 =
    b256!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");

/// Computes the Keccak-256 hash of the provided data.
///
/// This function is a thin wrapper around the Keccak256 hashing algorithm
/// and is optimized for performance.
///
/// # TODO
/// - Consider switching the return type to `B256` for consistency with other parts of the
///   codebase.
#[inline]
pub fn keccak(data: impl AsRef<[u8]>) -> [u8; 32] {
    // TODO: Remove this benchmarking code once performance testing is complete.
    // std::hint::black_box(sha2::Sha256::digest(&data));
    Keccak256::digest(data).into()
}
