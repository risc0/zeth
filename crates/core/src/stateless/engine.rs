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

use crate::stateless::data::StatelessClientData;
use crate::stateless::driver::SCEDriver;
use crate::stateless::execute::{DbExecutionInput, TransactionExecutionStrategy};
use crate::stateless::finalize::{FinalizationStrategy, MPTFinalizationInput};
use crate::stateless::initialize::{InitializationStrategy, MPTInitializationInput};
use crate::stateless::post_exec::PostExecutionValidationStrategy;
use crate::stateless::pre_exec::{ConsensusPreExecValidationInput, PreExecutionValidationStrategy};
use anyhow::Context;
use reth_chainspec::ChainSpec;
use reth_revm::db::BundleState;
use std::sync::Arc;

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct StatelessClientEngine<Block, Header, Database, Driver: SCEDriver<Block, Header>> {
    pub chain_spec: Arc<ChainSpec>,
    pub data: StatelessClientData<Block, Header>,
    pub db: Option<Database>,
    pub driver: Driver,
}

impl<Block, Header, Database, Driver: SCEDriver<Block, Header>>
    StatelessClientEngine<Block, Header, Database, Driver>
{
    /// Creates a new stateless validator
    pub fn new(
        chain_spec: Arc<ChainSpec>,
        data: StatelessClientData<Block, Header>,
        db: Option<Database>,
    ) -> Self {
        Self {
            chain_spec,
            data,
            db,
            driver: Driver::default(),
        }
    }

    /// Initializes the database from the input.
    pub fn initialize_database<
        T: for<'a> InitializationStrategy<
            Block,
            Header,
            Database,
            Input<'a> = MPTInitializationInput<'a, Header, Database>,
        >,
    >(
        &mut self,
    ) -> anyhow::Result<T::Output<'_>> {
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    parent_state_trie,
                    parent_storage,
                    contracts,
                    parent_header,
                    ancestor_headers,
                    ..
                },
            db,
            ..
        } = self;
        T::initialize_database((
            parent_state_trie,
            parent_storage,
            contracts,
            parent_header,
            ancestor_headers,
            db,
        ))
        .context("StatelessClientEngine::initialize_database")
    }

    /// Validates the header before execution.
    pub fn pre_execution_validation<
        T: for<'a> PreExecutionValidationStrategy<
            Block,
            Header,
            Database,
            Input<'a> = ConsensusPreExecValidationInput<'a, Block, Header>,
        >,
    >(
        &mut self,
    ) -> anyhow::Result<T::Output<'_>> {
        // Unpack input
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
        } = self;
        T::pre_execution_validation((chain_spec.clone(), block, parent_header, total_difficulty))
            .context("StatelessClientEngine::pre_execution_validation")
    }

    /// Executes transactions.
    pub fn execute_transactions<
        T: for<'a> TransactionExecutionStrategy<
            Block,
            Header,
            Database,
            Input<'a> = DbExecutionInput<'a, Block, Database>,
        >,
    >(
        &mut self,
    ) -> anyhow::Result<T::Output<'_>> {
        // Unpack input
        let StatelessClientEngine {
            chain_spec,
            data:
                StatelessClientData {
                    block,
                    total_difficulty,
                    ..
                },
            db,
            ..
        } = self;
        T::execute_transactions((chain_spec.clone(), block, total_difficulty, db))
            .context("StatelessClientEngine::execute_transactions")
    }

    /// Validates the header after execution.
    pub fn post_execution_validation<
        T: PostExecutionValidationStrategy<Block, Header, Database>,
    >(
        input: T::Input<'_>,
    ) -> anyhow::Result<T::Output<'_>> {
        T::post_execution_validation(input)
            .context("StatelessClientEngine::post_execution_validation")
    }

    /// Finalizes the state trie.
    pub fn finalize<
        T: for<'a> FinalizationStrategy<
            Block,
            Header,
            Database,
            Input<'a> = MPTFinalizationInput<'a, Block, Header>,
        >,
    >(
        &mut self,
        bundle_state: BundleState,
    ) -> anyhow::Result<T::Output> {
        // Unpack input
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    block,
                    parent_state_trie,
                    parent_storage,
                    parent_header,
                    total_difficulty,
                    ..
                },
            ..
        } = self;
        T::finalize((
            block,
            parent_state_trie,
            parent_storage,
            parent_header,
            total_difficulty,
            bundle_state,
        ))
        .context("StatelessClientEngine::finalize")
    }
}
