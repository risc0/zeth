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

use crate::stateless::client::StatelessClientEngine;
use crate::stateless::data::StatelessClientData;
use alloy_consensus::Header;
use alloy_primitives::Sealable;
use core::mem::take;
use reth_consensus::Consensus;
use reth_ethereum_consensus::EthBeaconConsensus;
use reth_primitives::{Block, SealedHeader};

pub trait PreExecutionValidationStrategy<Block, Header, Database> {
    type Output;
    fn pre_execution_validation(
        stateless_client_engine: &mut StatelessClientEngine<Block, Header, Database>,
    ) -> anyhow::Result<Self::Output>;
}

pub struct RethPreExecStrategy;

impl<Database> PreExecutionValidationStrategy<Block, Header, Database> for RethPreExecStrategy {
    type Output = ();

    fn pre_execution_validation(
        stateless_client_engine: &mut StatelessClientEngine<Block, Header, Database>,
    ) -> anyhow::Result<Self::Output> {
        // Unpack engine instance
        let StatelessClientEngine {
            chain_spec,
            data:
                StatelessClientData {
                    block,
                    parent_header,
                    total_difficulty,
                    ..
                },
            ..
        } = stateless_client_engine;
        // Instantiate consensus engine
        let consensus = EthBeaconConsensus::new(chain_spec.clone());
        // Validate total difficulty
        consensus.validate_header_with_total_difficulty(&block.header, *total_difficulty)?;
        // Validate header
        let sealed_block = take(block).seal_slow();
        consensus.validate_header(&sealed_block.header)?;
        // Validate header w.r.t. parent
        let sealed_parent_header = {
            let (parent_header, parent_header_seal) = take(parent_header).seal_slow().into_parts();
            SealedHeader::new(parent_header, parent_header_seal)
        };
        consensus.validate_header_against_parent(&sealed_block.header, &sealed_parent_header)?;
        // Check pre-execution block conditions
        consensus.validate_block_pre_execution(&sealed_block)?;
        // Return values
        *block = sealed_block.unseal();
        *parent_header = sealed_parent_header.unseal();
        Ok(())
    }
}
