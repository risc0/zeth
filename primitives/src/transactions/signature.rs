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

use alloy_primitives::{ChainId, U256};
use alloy_rlp_derive::RlpEncodable;
use serde::{Deserialize, Serialize};

/// Represents a cryptographic signature associated with a transaction.
///
/// The `TxSignature` struct encapsulates the components of an ECDSA signature: `v`, `r`,
/// and `s`. This signature can be used to recover the public key of the signer, ensuring
/// the authenticity of the transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, RlpEncodable)]
pub struct TxSignature {
    pub v: u64,
    pub r: U256,
    pub s: U256,
}

impl TxSignature {
    /// Returns the chain_id of the V value, if any.
    pub fn chain_id(&self) -> Option<ChainId> {
        match self.v {
            // EIP-155 encodes the chain_id in the V value
            value @ 35..=u64::MAX => Some((value - 35) / 2),
            _ => None,
        }
    }

    /// Computes the length of the RLP-encoded signature payload in bytes.
    pub fn payload_length(&self) -> usize {
        self._alloy_rlp_payload_length()
    }
}
