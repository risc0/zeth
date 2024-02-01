use core::fmt::Debug;

use anyhow::{bail, Context, Result};
use revm::{Database, DatabaseCommit};
use zeth_primitives::{block::Header, taiko::BLOCK_GAS_LIMIT, transactions::TxEssence};

use crate::{
    block_builder::BlockBuilder, consts::MAX_EXTRA_DATA_BYTES, preparation::HeaderPrepStrategy,
};

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
