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

use tiny_keccak::{Hasher, Keccak};

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
    let mut hasher = Keccak::v256();
    hasher.update(data.as_ref());
    let mut output = [0; 32];
    hasher.finalize(&mut output);
    output
}
