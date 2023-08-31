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

use alloy_primitives::U256;
use alloy_rlp_derive::{RlpEncodable, RlpMaxEncodedLen};
use serde::{Deserialize, Serialize};

/// Represents a cryptographic signature associated with a transaction.
///
/// The `TxSignature` struct encapsulates the components of an ECDSA signature: `v`, `r`,
/// and `s`. This signature can be used to recover the public key of the signer, ensuring
/// the authenticity of the transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, RlpMaxEncodedLen)]
pub struct TxSignature {
    pub v: u64,
    pub r: U256,
    pub s: U256,
}

impl TxSignature {
    /// Computes the length of the RLP-encoded signature payload in bytes.
    pub fn payload_length(&self) -> usize {
        self._alloy_rlp_payload_length()
    }
}
