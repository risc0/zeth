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

//! Constants for the Ethereum protocol.
extern crate alloc;

use std::collections::BTreeMap;

use anyhow::{bail, Result};
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

/// The Ethereum mainnet specification.
pub static ETH_MAINNET_CHAIN_SPEC: Lazy<ChainSpec> = Lazy::new(|| ChainSpec {
    chain_id: 1,
    max_spec_id: SpecId::SHANGHAI,
    hard_forks: BTreeMap::from([
        (SpecId::MERGE, ForkCondition::Block(15537394)),
        (SpecId::SHANGHAI, ForkCondition::Timestamp(1681338455)),
        (SpecId::CANCUN, ForkCondition::Timestamp(1710338135)),
    ]),
    gas_constants: BTreeMap::from([(SpecId::LONDON, ETH_MAINNET_EIP1559_CONSTANTS)]),
});

/// The Ethereum mainnet EIP-1559 gas constants.
pub const ETH_MAINNET_EIP1559_CONSTANTS: Eip1559Constants = Eip1559Constants {
    base_fee_change_denominator: uint!(8_U256),
    base_fee_max_increase_denominator: uint!(8_U256),
    base_fee_max_decrease_denominator: uint!(8_U256),
    elasticity_multiplier: uint!(2_U256),
};

/// The Optimism mainnet specification.
pub static OP_MAINNET_CHAIN_SPEC: Lazy<ChainSpec> = Lazy::new(|| ChainSpec {
    chain_id: 10,
    max_spec_id: SpecId::CANYON,
    hard_forks: BTreeMap::from([
        (SpecId::BEDROCK, ForkCondition::Timestamp(1679079600)),
        // Regolith is activated from day 1 of Bedrock on mainnet
        (SpecId::REGOLITH, ForkCondition::Timestamp(1679079600)),
        // Canyon is activated 2024-01-11 at 17:00:01 UTC
        (SpecId::CANYON, ForkCondition::Timestamp(1704992401)),
        // Delta is activated 2024-02-22 at 17:00:01 UTC
        (SpecId::LATEST, ForkCondition::Timestamp(1708560000)),
    ]),
    gas_constants: BTreeMap::from([
        (
            SpecId::BEDROCK,
            Eip1559Constants {
                base_fee_change_denominator: uint!(50_U256),
                base_fee_max_increase_denominator: uint!(10_U256),
                base_fee_max_decrease_denominator: uint!(50_U256),
                elasticity_multiplier: uint!(6_U256),
            },
        ),
        (
            SpecId::CANYON,
            Eip1559Constants {
                base_fee_change_denominator: uint!(250_U256),
                base_fee_max_increase_denominator: uint!(10_U256),
                base_fee_max_decrease_denominator: uint!(50_U256),
                elasticity_multiplier: uint!(6_U256),
            },
        ),
    ]),
});

/// The condition at which a fork is activated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ForkCondition {
    /// The fork is activated with a certain block.
    Block(BlockNumber),
    /// The fork is activated with a specific timestamp.
    Timestamp(u64),
    /// The fork is never activated
    #[default]
    TBD,
}

impl ForkCondition {
    /// Returns whether the condition has been met.
    pub fn active(&self, block_number: BlockNumber, timestamp: u64) -> bool {
        match self {
            ForkCondition::Block(block) => *block <= block_number,
            ForkCondition::Timestamp(ts) => *ts <= timestamp,
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

/// Specification of a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSpec {
    chain_id: ChainId,
    max_spec_id: SpecId,
    hard_forks: BTreeMap<SpecId, ForkCondition>,
    gas_constants: BTreeMap<SpecId, Eip1559Constants>,
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
            max_spec_id: spec_id,
            hard_forks: BTreeMap::from([(spec_id, ForkCondition::Block(0))]),
            gas_constants: BTreeMap::from([(spec_id, eip_1559_constants)]),
        }
    }
    /// Returns the network chain ID.
    pub fn chain_id(&self) -> ChainId {
        self.chain_id
    }
    /// Returns the [SpecId] for a given block number and timestamp or an error if not
    /// supported.
    pub fn active_fork(&self, block_number: BlockNumber, timestamp: &U256) -> Result<SpecId> {
        match self.spec_id(block_number, timestamp.saturating_to()) {
            Some(spec_id) => {
                if spec_id > self.max_spec_id {
                    bail!("expected <= {:?}, got {:?}", self.max_spec_id, spec_id);
                } else {
                    Ok(spec_id)
                }
            }
            None => bail!("no supported fork for block {}", block_number),
        }
    }
    /// Returns the Eip1559 constants for a given [SpecId].
    pub fn gas_constants(&self, spec_id: SpecId) -> Option<&Eip1559Constants> {
        self.gas_constants
            .range(..=spec_id)
            .next_back()
            .map(|(_, v)| v)
    }

    fn spec_id(&self, block_number: BlockNumber, timestamp: u64) -> Option<SpecId> {
        for (spec_id, fork) in self.hard_forks.iter().rev() {
            if fork.active(block_number, timestamp) {
                return Some(*spec_id);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_id() {
        assert_eq!(ETH_MAINNET_CHAIN_SPEC.spec_id(15537393, 0), None);
        assert_eq!(
            ETH_MAINNET_CHAIN_SPEC.spec_id(15537394, 0),
            Some(SpecId::MERGE)
        );
        assert_eq!(
            ETH_MAINNET_CHAIN_SPEC.spec_id(17034869, 0),
            Some(SpecId::MERGE)
        );
        assert_eq!(
            ETH_MAINNET_CHAIN_SPEC.spec_id(0, 1681338455),
            Some(SpecId::SHANGHAI)
        );
    }

    #[test]
    fn gas_constants() {
        assert_eq!(ETH_MAINNET_CHAIN_SPEC.gas_constants(SpecId::BERLIN), None);
        assert_eq!(
            ETH_MAINNET_CHAIN_SPEC.gas_constants(SpecId::MERGE),
            Some(&ETH_MAINNET_EIP1559_CONSTANTS)
        );
        assert_eq!(
            ETH_MAINNET_CHAIN_SPEC.gas_constants(SpecId::SHANGHAI),
            Some(&ETH_MAINNET_EIP1559_CONSTANTS)
        );
    }
}
