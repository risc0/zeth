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

use alloy_primitives::B160;
use alloy_rlp_derive::RlpEncodable;
use serde::{Deserialize, Serialize};

/// A validator withdrawal from the consensus layer ([EIP-4895](https://eips.ethereum.org/EIPS/eip-4895)).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, RlpEncodable)]
pub struct Withdrawal {
    /// Monotonically increasing identifier assigned by consensus layer.
    pub index: u64,
    /// Index of validator associated with withdrawal.
    pub validator_index: u64,
    /// Target address for withdrawn ether.
    pub address: B160,
    /// Value of the withdrawal in gwei.
    pub amount: u64,
}
