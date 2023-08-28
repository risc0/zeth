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

use alloy_primitives::Address;
use alloy_rlp_derive::{RlpEncodable, RlpMaxEncodedLen};
use serde::{Deserialize, Serialize};

/// Represents a validator's withdrawal from the Ethereum consensus layer.
///
/// The `Withdrawal` struct provides a model for the process a validator undergoes when
/// withdrawing funds from the Ethereum consensus mechanism. This process is outlined in
/// detail in [EIP-4895](https://eips.ethereum.org/EIPS/eip-4895). Each `Withdrawal` instance carries
/// specific identifiers and target details to ensure the accurate and secure transfer of
/// ether.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, RlpMaxEncodedLen,
)]
pub struct Withdrawal {
    /// A unique, monotonically increasing identifier assigned by the consensus layer to
    /// distinctly represent this withdrawal.
    pub index: u64,
    /// The distinct index of the validator initiating this withdrawal.
    pub validator_index: u64,
    /// The Ethereum address, encapsulated as a `Address` type, where the withdrawn ether
    /// will be sent.
    pub address: Address,
    /// The total withdrawal amount, denominated in gwei.
    pub amount: u64,
}
