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

use alloy_consensus::Header;
use alloy_primitives::{Sealable, U256};
use anyhow::Context;
use core::mem::take;
use reth_chainspec::ChainSpec;
use reth_consensus::Consensus;
use reth_ethereum_consensus::EthBeaconConsensus;
use reth_primitives::{Block, SealedHeader};
use std::sync::Arc;

pub trait PreExecutionValidationStrategy<Block, Header, Database> {
    type Input<'a>;
    type Output<'b>;
    fn pre_execution_validation(input: Self::Input<'_>) -> anyhow::Result<Self::Output<'_>>;
}

pub struct RethPreExecStrategy;
pub type ConsensusPreExecValidationInput<'a, B, H> =
    (Arc<ChainSpec>, &'a mut B, &'a mut H, &'a mut U256);

impl<Database> PreExecutionValidationStrategy<Block, Header, Database> for RethPreExecStrategy
where
    Database: 'static,
{
    type Input<'a> = ConsensusPreExecValidationInput<'a, Block, Header>;
    type Output<'b> = ();

    fn pre_execution_validation(
        (chain_spec, block, parent_header, total_difficulty): Self::Input<'_>,
    ) -> anyhow::Result<Self::Output<'_>> {
        // Instantiate consensus engine
        let consensus = EthBeaconConsensus::new(chain_spec);
        // Validate total difficulty
        consensus
            .validate_header_with_total_difficulty(&block.header, *total_difficulty)
            .context("validate_header_with_total_difficulty")?;
        // Validate header (todo: seal beforehand to save rehashing costs)
        let sealed_block = take(block).seal_slow();
        consensus
            .validate_header(&sealed_block.header)
            .context("validate_header")?;
        // Validate header w.r.t. parent
        let sealed_parent_header = {
            let (parent_header, parent_header_seal) = take(parent_header).seal_slow().into_parts();
            SealedHeader::new(parent_header, parent_header_seal)
        };
        consensus
            .validate_header_against_parent(&sealed_block.header, &sealed_parent_header)
            .context("validate_header_against_parent")?;
        // Check pre-execution block conditions
        consensus
            .validate_block_pre_execution(&sealed_block)
            .context("validate_block_pre_execution")?;
        // Return values
        *block = sealed_block.unseal();
        *parent_header = sealed_parent_header.unseal();
        Ok(())
    }
}
