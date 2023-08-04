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

//! Constants for the Ethereum protocol.

use std::collections::BTreeMap;

use once_cell::sync::Lazy;
use revm::primitives::SpecId;
use serde::{Deserialize, Serialize};
use zeth_primitives::{uint, BlockNumber, ChainId, U256};

/// U256 representation of 0.
pub const ZERO: U256 = U256::ZERO;
/// U256 representation of 1.
pub const ONE: U256 = uint!(1_U256);

/// The bound divisor of the gas limit,
pub const GAS_LIMIT_BOUND_DIVISOR: U256 = uint!(1024_U256);
/// Minimum the gas limit may ever be.
pub const MIN_GAS_LIMIT: U256 = uint!(5000_U256);

/// Maximum size of extra data.
pub const MAX_EXTRA_DATA_BYTES: usize = 32;

/// Maximum allowed block number difference for the `block_hash` call.
pub const MAX_BLOCK_HASH_AGE: u64 = 256;

/// Multiplier for converting gwei to wei.
pub const GWEI_TO_WEI: U256 = uint!(1_000_000_000_U256);

/// [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559) parameter.
pub const BASE_FEE_MAX_CHANGE_DENOMINATOR: U256 = uint!(8_U256);
/// [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559) parameter.
pub const ELASTICITY_MULTIPLIER: U256 = uint!(2_U256);

/// Minimum supported protocol version: Paris (Block no. 15537394).
pub const MIN_SPEC_ID: SpecId = SpecId::MERGE;

/// The Ethereum mainnet specification.
pub static MAINNET: Lazy<ChainSpec> = Lazy::new(|| {
    ChainSpec {
        chain_id: 1,
        hard_forks: BTreeMap::from([
            (SpecId::FRONTIER, ForkCondition::Block(0)),
            // previous versions not supported
            (SpecId::MERGE, ForkCondition::Block(15537394)),
            (SpecId::SHANGHAI, ForkCondition::Block(17034870)),
            (SpecId::CANCUN, ForkCondition::TBD),
        ]),
    }
});

/// The condition at which a fork is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ForkCondition {
    /// The fork is activated with a certain block.
    Block(BlockNumber),
    /// The fork is not yet active.
    TBD,
}

impl ForkCondition {
    /// Returns wether the condition has been met.
    pub fn active(&self, block_number: BlockNumber) -> bool {
        match self {
            ForkCondition::Block(block) => *block <= block_number,
            ForkCondition::TBD => false,
        }
    }
}

/// Specification of a specific chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainSpec {
    chain_id: ChainId,
    hard_forks: BTreeMap<SpecId, ForkCondition>,
}

impl ChainSpec {
    /// Creates a new configuration consisting of only one specification ID.
    pub fn new_single(chain_id: ChainId, spec_id: SpecId) -> Self {
        ChainSpec {
            chain_id,
            hard_forks: BTreeMap::from([(spec_id, ForkCondition::Block(0))]),
        }
    }
    /// Returns the network chain ID.
    pub fn chain_id(&self) -> ChainId {
        self.chain_id
    }
    /// Returns the revm specification ID for `block_number`.
    pub fn spec_id(&self, block_number: BlockNumber) -> SpecId {
        for (spec_id, fork) in self.hard_forks.iter().rev() {
            if fork.active(block_number) {
                return *spec_id;
            }
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revm_spec_id() {
        assert!(MAINNET.spec_id(15537393) < SpecId::MERGE);
        assert_eq!(MAINNET.spec_id(15537394), SpecId::MERGE);
        assert_eq!(MAINNET.spec_id(17034869), SpecId::MERGE);
        assert_eq!(MAINNET.spec_id(17034870), SpecId::SHANGHAI);
    }
}
