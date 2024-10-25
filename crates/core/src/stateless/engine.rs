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
use crate::rescue::{Recoverable, Rescued, Wrapper};

/// A generic builder for building a block.
pub struct StatelessClientEngine<Block, Header, Database: Recoverable, Driver: SCEDriver<Block, Header>> {
    pub chain_spec: Arc<ChainSpec>,
    pub data: StatelessClientData<Block, Header>,
    pub db: Option<Wrapper<Database>>,
    pub db_rescued: Option<Rescued<Database>>,
    pub driver: Driver,
}

impl<Block, Header, Database: Recoverable, Driver: SCEDriver<Block, Header>>
    StatelessClientEngine<Block, Header, Database, Driver>
{
    /// Creates a new stateless validator
    pub fn new(
        chain_spec: Arc<ChainSpec>,
        data: StatelessClientData<Block, Header>,
        db: Option<Database>,
    ) -> Self {
        let db = db.map(|db| Wrapper::from(db));
        let db_rescued = db.as_ref().map(|db| db.rescued());
        Self {
            chain_spec,
            data,
            db,
            db_rescued,
            driver: Driver::default(),
        }
    }

    /// Initializes the database from the input.
    pub fn initialize_database<
        T: for<'a, 'b> InitializationStrategy<
            Block,
            Header,
            Database,
            Input<'a> = MPTInitializationInput<'a, Header>,
            Output<'b> = Database,
        >,
    >(
        &mut self,
    ) -> anyhow::Result<Option<Database>> {
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    state_trie,
                    storage_tries,
                    contracts,
                    parent_header,
                    ancestor_headers,
                    ..
                },
            db,
            ..
        } = self;
        let new_db = Wrapper::from(T::initialize_database((
            state_trie,
            storage_tries,
            contracts,
            parent_header,
            ancestor_headers,
        ))
            .context("StatelessClientEngine::initialize_database")?);
        self.db_rescued = Some(new_db.rescued());
        Ok(db
            .replace(new_db)
            .map(|mut rescue_db| rescue_db.rescue())
            .flatten())
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
                    blocks,
                    parent_header,
                    total_difficulty,
                    ..
                },
            ..
        } = self;
        T::pre_execution_validation((
            chain_spec.clone(),
            blocks.last_mut().unwrap(),
            parent_header,
            total_difficulty,
        ))
        .context("StatelessClientEngine::pre_execution_validation")
    }

    /// Executes transactions.
    pub fn execute_transactions<
        T: for<'a> TransactionExecutionStrategy<
            Block,
            Header,
            Wrapper<Database>,
            Input<'a> = DbExecutionInput<'a, Block, Wrapper<Database>>,
        >,
    >(
        &mut self,
    ) -> anyhow::Result<T::Output<'_>> {
        // Unpack input
        let StatelessClientEngine {
            chain_spec,
            data:
                StatelessClientData {
                    blocks,
                    total_difficulty,
                    ..
                },
            db,
            ..
        } = self;
        T::execute_transactions((
            chain_spec.clone(),
            blocks.last_mut().unwrap(),
            total_difficulty,
            db,
        ))
        .context("StatelessClientEngine::execute_transactions")
    }

    /// Validates the header after execution.
    pub fn post_execution_validation<
        T: PostExecutionValidationStrategy<Block, Header, Wrapper<Database>>,
    >(
        input: T::Input<'_>,
    ) -> anyhow::Result<T::Output<'_>> {
        let output = T::post_execution_validation(input)
            .context("StatelessClientEngine::post_execution_validation")?;

        Ok(output)
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
                    blocks,
                    state_trie,
                    storage_tries,
                    parent_header,
                    total_difficulty,
                    ..
                },
            ..
        } = self;
        // Follow finalization strategy
        let result = T::finalize((
            blocks.last_mut().unwrap(),
            state_trie,
            storage_tries,
            parent_header,
            bundle_state,
        ))
        .context("StatelessClientEngine::finalize")?;
        // Prepare for next block
        *parent_header = Driver::block_to_header(blocks.pop().unwrap());
        *total_difficulty = Driver::accumulate_difficulty(*total_difficulty, &*parent_header);

        Ok(result)
    }
}
