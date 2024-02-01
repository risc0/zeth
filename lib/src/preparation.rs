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
use revm::{Database, DatabaseCommit};
use zeth_primitives::{block::Header, transactions::TxEssence, U256};

use crate::{
    block_builder::BlockBuilder,
    consts::{Eip1559Constants, GAS_LIMIT_BOUND_DIVISOR, MAX_EXTRA_DATA_BYTES, MIN_GAS_LIMIT, ONE},
};

pub trait HeaderPrepStrategy {
    fn prepare_header<D, E>(block_builder: BlockBuilder<D, E>) -> Result<BlockBuilder<D, E>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
        E: TxEssence;
}

pub struct EthHeaderPrepStrategy {}

impl HeaderPrepStrategy for EthHeaderPrepStrategy {
    fn prepare_header<D, E>(mut block_builder: BlockBuilder<D, E>) -> Result<BlockBuilder<D, E>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
        E: TxEssence,
    {
        // Validate gas limit
        let diff = block_builder
            .input
            .parent_header
            .gas_limit
            .abs_diff(block_builder.input.gas_limit);
        let limit = block_builder.input.parent_header.gas_limit / GAS_LIMIT_BOUND_DIVISOR;
        if diff >= limit {
            bail!(
                "Invalid gas limit: expected {} +- {}, got {}",
                block_builder.input.parent_header.gas_limit,
                limit,
                block_builder.input.gas_limit,
            );
        }
        if block_builder.input.gas_limit < MIN_GAS_LIMIT {
            bail!(
                "Invalid gas limit: expected >= {}, got {}",
                MIN_GAS_LIMIT,
                block_builder.input.gas_limit,
            );
        }
        // Validate timestamp
        if block_builder.input.timestamp <= block_builder.input.parent_header.timestamp {
            bail!(
                "Invalid timestamp: expected > {}, got {}",
                block_builder.input.parent_header.timestamp,
                block_builder.input.timestamp,
            );
        }
        // Validate extra data
        let extra_data_bytes = block_builder.input.extra_data.len();
        if extra_data_bytes > MAX_EXTRA_DATA_BYTES {
            bail!(
                "Invalid extra data: expected <= {}, got {}",
                MAX_EXTRA_DATA_BYTES,
                extra_data_bytes,
            )
        }
        // Derive header
        block_builder.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: block_builder.input.parent_header.hash(),
            number: block_builder
                .input
                .parent_header
                .number
                .checked_add(1)
                .context("Invalid block number: too large")?,
            base_fee_per_gas: derive_base_fee(
                &block_builder.input.parent_header,
                block_builder.chain_spec.gas_constants(),
            )?,
            // Initialize metadata from input
            beneficiary: block_builder.input.beneficiary,
            gas_limit: block_builder.input.gas_limit,
            timestamp: block_builder.input.timestamp,
            mix_hash: block_builder.input.mix_hash,
            extra_data: block_builder.input.extra_data.clone(),
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(block_builder)
    }
}

/// Base fee for next block. [EIP-1559](https://github.com/ethereum/EIPs/blob/master/EIPS/eip-1559.md) spec
#[inline(always)]
pub fn derive_base_fee(parent: &Header, eip_1559_constants: &Eip1559Constants) -> Result<U256> {
    let parent_gas_target = parent.gas_limit / eip_1559_constants.elasticity_multiplier;

    match parent.gas_used.cmp(&parent_gas_target) {
        core::cmp::Ordering::Equal => Ok(parent.base_fee_per_gas),

        core::cmp::Ordering::Greater => {
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

        core::cmp::Ordering::Less => {
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
