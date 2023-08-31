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

use core::str::FromStr;
use std::collections::BTreeMap;

use anyhow::bail;
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

/// Minimum supported protocol version: Paris (Block no. 15537394).
pub const MIN_SPEC_ID: SpecId = SpecId::MERGE;

/// The Ethereum mainnet specification.
pub static ETH_MAINNET_CHAIN_SPEC: Lazy<ChainSpec> = Lazy::new(|| {
    ChainSpec {
        chain_id: 1,
        hard_forks: BTreeMap::from([
            (SpecId::FRONTIER, ForkCondition::Block(0)),
            // previous versions not supported
            (SpecId::MERGE, ForkCondition::Block(15537394)),
            (SpecId::SHANGHAI, ForkCondition::Block(17034870)),
            (SpecId::CANCUN, ForkCondition::TBD),
        ]),
        eip_1559_constants: Eip1559Constants {
            base_fee_change_denominator: uint!(8_U256),
            base_fee_max_increase_denominator: uint!(8_U256),
            base_fee_max_decrease_denominator: uint!(8_U256),
            elasticity_multiplier: uint!(2_U256),
        },
    }
});

/// The optimism mainnet specification.
pub static OP_MAINNET_CHAIN_SPEC: Lazy<ChainSpec> = Lazy::new(|| ChainSpec {
    chain_id: 10,
    hard_forks: BTreeMap::from([(SpecId::MERGE, ForkCondition::Block(0))]),
    eip_1559_constants: Eip1559Constants {
        base_fee_change_denominator: uint!(50_U256),
        base_fee_max_increase_denominator: uint!(10_U256),
        base_fee_max_decrease_denominator: uint!(50_U256),
        elasticity_multiplier: uint!(6_U256),
    },
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
    /// Returns whether the condition has been met.
    pub fn active(&self, block_number: BlockNumber) -> bool {
        match self {
            ForkCondition::Block(block) => *block <= block_number,
            ForkCondition::TBD => false,
        }
    }
}

/// [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559) parameters.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct Eip1559Constants {
    pub base_fee_change_denominator: U256,
    pub base_fee_max_increase_denominator: U256,
    pub base_fee_max_decrease_denominator: U256,
    pub elasticity_multiplier: U256,
}

impl Default for Eip1559Constants {
    /// Defaults to Ethereum network values
    fn default() -> Self {
        Self {
            base_fee_change_denominator: uint!(8_U256),
            base_fee_max_increase_denominator: uint!(8_U256),
            base_fee_max_decrease_denominator: uint!(8_U256),
            elasticity_multiplier: uint!(2_U256),
        }
    }
}

/// Specification of a specific chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainSpec {
    chain_id: ChainId,
    hard_forks: BTreeMap<SpecId, ForkCondition>,
    eip_1559_constants: Eip1559Constants,
}

impl ChainSpec {
    /// Creates a new configuration consisting of only one specification ID.
    pub fn new_single(
        chain_id: ChainId,
        spec_id: SpecId,
        eip_1559_constants: Eip1559Constants,
    ) -> Self {
        ChainSpec {
            chain_id,
            hard_forks: BTreeMap::from([(spec_id, ForkCondition::Block(0))]),
            eip_1559_constants,
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
    /// Returns the Eip1559 constants
    pub fn gas_constants(&self) -> &Eip1559Constants {
        &self.eip_1559_constants
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Network {
    #[default]
    Ethereum,
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ethereum" => Ok(Network::Ethereum),
            _ => bail!("Unknown network"),
        }
    }
}

impl ToString for Network {
    fn to_string(&self) -> String {
        match self {
            Network::Ethereum => String::from("ethereum"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revm_spec_id() {
        assert!(ETH_MAINNET_CHAIN_SPEC.spec_id(15537393) < SpecId::MERGE);
        assert_eq!(ETH_MAINNET_CHAIN_SPEC.spec_id(15537394), SpecId::MERGE);
        assert_eq!(ETH_MAINNET_CHAIN_SPEC.spec_id(17034869), SpecId::MERGE);
        assert_eq!(ETH_MAINNET_CHAIN_SPEC.spec_id(17034870), SpecId::SHANGHAI);
    }
}
