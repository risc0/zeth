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

use core::fmt::Debug;

use anyhow::{bail, Context, Result};
use revm::{Database, DatabaseCommit};
use zeth_primitives::block::Header;

use crate::{
    builder::BlockBuilder,
    consts::{Eip1559Constants, GAS_LIMIT_BOUND_DIVISOR, MAX_EXTRA_DATA_BYTES, MIN_GAS_LIMIT},
};

pub trait HeaderPrepStrategy {
    fn prepare_header<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: core::fmt::Debug;
}

pub struct EthHeaderPrepStrategy {}

impl HeaderPrepStrategy for EthHeaderPrepStrategy {
    fn prepare_header<D>(mut block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
    {
        // Validate gas limit
        let diff = block_builder
            .input
            .state_input
            .parent_header
            .gas_limit
            .abs_diff(block_builder.input.state_input.gas_limit);
        let limit =
            block_builder.input.state_input.parent_header.gas_limit / GAS_LIMIT_BOUND_DIVISOR;
        if diff >= limit {
            bail!(
                "Invalid gas limit: expected {} +- {}, got {}",
                block_builder.input.state_input.parent_header.gas_limit,
                limit,
                block_builder.input.state_input.gas_limit,
            );
        }
        if block_builder.input.state_input.gas_limit < MIN_GAS_LIMIT {
            bail!(
                "Invalid gas limit: expected >= {}, got {}",
                MIN_GAS_LIMIT,
                block_builder.input.state_input.gas_limit,
            );
        }
        // Validate timestamp
        let timestamp = block_builder.input.state_input.timestamp;
        if timestamp <= block_builder.input.state_input.parent_header.timestamp {
            bail!(
                "Invalid timestamp: expected > {}, got {}",
                block_builder.input.state_input.parent_header.timestamp,
                block_builder.input.state_input.timestamp,
            );
        }
        // Validate extra data
        let extra_data_bytes = block_builder.input.state_input.extra_data.len();
        if extra_data_bytes > MAX_EXTRA_DATA_BYTES {
            bail!(
                "Invalid extra data: expected <= {}, got {}",
                MAX_EXTRA_DATA_BYTES,
                extra_data_bytes,
            )
        }
        // Validate number
        let parent_number = block_builder.input.state_input.parent_header.number;
        let number = parent_number
            .checked_add(1)
            .context("Invalid number: too large")?;

        // Derive fork version
        let spec_id = block_builder
            .chain_spec
            .active_fork(number, timestamp)
            .unwrap_or_else(|err| panic!("Invalid version: {:#}", err));
        block_builder.spec_id = Some(spec_id);
        // Derive header
        block_builder.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: block_builder.input.state_input.parent_header.hash_slow(),
            number: block_builder
                .input
                .state_input
                .parent_header
                .number
                .checked_add(1)
                .context("Invalid block number: too large")?,
            base_fee_per_gas: Some(derive_base_fee(
                &block_builder.input.state_input.parent_header,
                block_builder.chain_spec.gas_constants(spec_id).unwrap(),
            )),
            // Initialize metadata from input
            beneficiary: block_builder.input.state_input.beneficiary,
            gas_limit: block_builder.input.state_input.gas_limit,
            timestamp,
            mix_hash: block_builder.input.state_input.mix_hash,
            extra_data: block_builder.input.state_input.extra_data.clone(),
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(block_builder)
    }
}

/// Base fee for next block. [EIP-1559](https://github.com/ethereum/EIPs/blob/master/EIPS/eip-1559.md) spec
fn derive_base_fee(parent: &Header, eip_1559_constants: &Eip1559Constants) -> u64 {
    let parent_gas_target = parent.gas_limit / eip_1559_constants.elasticity_multiplier;
    let parent_base_fee = parent.base_fee_per_gas.unwrap();

    match parent.gas_used.cmp(&parent_gas_target) {
        std::cmp::Ordering::Equal => parent_base_fee,

        std::cmp::Ordering::Greater => {
            // calculate the increase in base fee based on the formula defined by EIP-1559
            let gas_used_delta = parent.gas_used - parent_gas_target;
            let base_fee_delta = 1u64.max(
                (parent_base_fee as u128 * gas_used_delta as u128
                    / (parent_gas_target as u128
                        * eip_1559_constants.base_fee_change_denominator as u128))
                    as u64,
            );
            parent_base_fee + base_fee_delta
        }

        std::cmp::Ordering::Less => {
            // calculate the decrease in base fee based on the formula defined by EIP-1559
            let gas_used_delta = parent_gas_target - parent.gas_used;
            let base_fee_delta = (parent_base_fee as u128 * gas_used_delta as u128
                / (parent_gas_target as u128
                    * eip_1559_constants.base_fee_change_denominator as u128))
                as u64;
            parent_base_fee.saturating_sub(base_fee_delta)
        }
    }
}
