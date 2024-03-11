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
use zeth_primitives::{block::Header, transactions::TxEssence};

use crate::{builder::BlockBuilder, consts::MAX_EXTRA_DATA_BYTES, taiko_utils::BLOCK_GAS_LIMIT};

pub trait HeaderPrepStrategy {
    fn prepare_header<D, E>(block_builder: BlockBuilder<D, E>) -> Result<BlockBuilder<D, E>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: core::fmt::Debug,
        E: TxEssence;
}

pub struct TaikoHeaderPrepStrategy {}

impl HeaderPrepStrategy for TaikoHeaderPrepStrategy {
    fn prepare_header<D, E>(mut block_builder: BlockBuilder<D, E>) -> Result<BlockBuilder<D, E>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
        E: TxEssence,
    {
        // Validate gas limit
        if block_builder.input.gas_limit != *BLOCK_GAS_LIMIT {
            bail!(
                "Invalid gas limit: expected == {}, got {}",
                *BLOCK_GAS_LIMIT,
                block_builder.input.gas_limit,
            );
        }
        // Validate timestamp
        if block_builder.input.timestamp < block_builder.input.parent_header.timestamp {
            bail!(
                "Invalid timestamp: expected >= {}, got {}",
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
                .with_context(|| "Invalid block number: too large")?,
            base_fee_per_gas: block_builder.input.base_fee_per_gas,
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
