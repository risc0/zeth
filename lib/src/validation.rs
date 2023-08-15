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

use core::fmt::Debug;

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use revm::primitives::{SpecId, B160 as RevmB160};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header, transaction::Transaction, trie::MptNode, withdrawal::Withdrawal, BlockNumber,
    Bytes, B160, B256, U256,
};

use crate::consts::{
    ChainSpec, Eip1559Constants, GAS_LIMIT_BOUND_DIVISOR, MAX_EXTRA_DATA_BYTES, MIN_GAS_LIMIT,
    MIN_SPEC_ID, ONE,
};

/// External block input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Input {
    /// Previous block header
    pub parent_header: Header,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: B160,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: U256,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: U256,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    pub transactions: Vec<Transaction>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<Withdrawal>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<RevmB160, MptNode>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

pub fn verify_gas_limit(input_gas_limit: U256, parent_gas_limit: U256) -> Result<()> {
    let diff = parent_gas_limit.abs_diff(input_gas_limit);
    let limit = parent_gas_limit / GAS_LIMIT_BOUND_DIVISOR;
    if diff >= limit {
        bail!(
            "Invalid gas limit: expected {} +- {}, got {}",
            parent_gas_limit,
            limit,
            input_gas_limit,
        );
    }
    if input_gas_limit < MIN_GAS_LIMIT {
        bail!(
            "Invalid gas limit: expected >= {}, got {}",
            MIN_GAS_LIMIT,
            input_gas_limit,
        );
    }

    Ok(())
}

pub fn verify_timestamp(input_timestamp: U256, parent_timestamp: U256) -> Result<()> {
    if input_timestamp <= parent_timestamp {
        bail!(
            "Invalid timestamp: expected > {}, got {}",
            parent_timestamp,
            input_timestamp,
        );
    }

    Ok(())
}

pub fn verify_extra_data(input_extra_data: &Bytes) -> Result<()> {
    let extra_data_bytes = input_extra_data.len();
    if extra_data_bytes >= MAX_EXTRA_DATA_BYTES {
        bail!(
            "Invalid extra data: expected <= {}, got {}",
            MAX_EXTRA_DATA_BYTES,
            extra_data_bytes,
        )
    }

    Ok(())
}

pub fn compute_block_number(parent: &Header) -> Result<BlockNumber> {
    parent
        .number
        .checked_add(1)
        .context("Invalid block number: too large")
}

pub fn compute_spec_id(chain_spec: &ChainSpec, block_number: BlockNumber) -> Result<SpecId> {
    let spec_id = chain_spec.spec_id(block_number);
    if !SpecId::enabled(spec_id, MIN_SPEC_ID) {
        bail!(
            "Invalid protocol version: expected >= {:?}, got {:?}",
            MIN_SPEC_ID,
            spec_id,
        )
    }
    Ok(spec_id)
}

/// Calculate base fee for next block. [EIP-1559](https://github.com/ethereum/EIPs/blob/master/EIPS/eip-1559.md) spec
pub fn compute_base_fee(parent: &Header, eip_1559_constants: &Eip1559Constants) -> Result<U256> {
    let parent_gas_target = parent.gas_limit / eip_1559_constants.elasticity_multiplier;

    match parent.gas_used.cmp(&parent_gas_target) {
        std::cmp::Ordering::Equal => Ok(parent.base_fee_per_gas),

        std::cmp::Ordering::Greater => {
            let gas_used_delta = parent.gas_used - parent_gas_target;
            let base_fee_delta = ONE
                .max(
                    parent.base_fee_per_gas * gas_used_delta
                        / parent_gas_target
                        / eip_1559_constants.base_fee_change_denominator,
                )
                .min(
                    parent.base_fee_per_gas / eip_1559_constants.base_fee_max_increase_denominator,
                );
            Ok(parent.base_fee_per_gas + base_fee_delta)
        }

        std::cmp::Ordering::Less => {
            let gas_used_delta = parent_gas_target - parent.gas_used;
            let base_fee_delta = (parent.base_fee_per_gas * gas_used_delta
                / parent_gas_target
                / eip_1559_constants.base_fee_change_denominator)
                .min(
                    parent.base_fee_per_gas / eip_1559_constants.base_fee_max_decrease_denominator,
                );
            Ok(parent.base_fee_per_gas - base_fee_delta)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = Input::default();
        let _: Input = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
