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
use crate::stateless::execute::{DbExecutionInput, RethExecStrategy, TransactionExecutionStrategy};
use crate::stateless::finalize::{
    FinalizationStrategy, MPTFinalizationInput, RethFinalizationStrategy,
};
use crate::stateless::initialize::{
    InMemoryDbStrategy, InitializationStrategy, MPTInitializationInput,
};
use crate::stateless::post_exec::{PostExecutionValidationStrategy, RethPostExecStrategy};
use crate::stateless::pre_exec::{
    ConsensusPreExecValidationInput, PreExecutionValidationStrategy, RethPreExecStrategy,
};
use alloy_consensus::Header;
use anyhow::Context;
use reth_chainspec::ChainSpec;
use reth_primitives::Block;
use reth_revm::db::BundleState;
use reth_revm::InMemoryDB;
use std::sync::{Arc, Mutex};

pub type RescueDestination<D> = Arc<Mutex<Option<D>>>;

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct StatelessClientEngine<Block, Header, Database> {
    pub chain_spec: Arc<ChainSpec>,
    pub data: StatelessClientData<Block, Header>,
    pub db: Option<Database>,
}

impl<Block, Header, Database> StatelessClientEngine<Block, Header, Database> {
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

pub trait StatelessClient<Block: 'static, Header: 'static, Database: 'static> {
    type Initialization: for<'a> InitializationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = MPTInitializationInput<'a, Header, Database>,
    >;
    type PreExecValidation: for<'a> PreExecutionValidationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = ConsensusPreExecValidationInput<'a, Block, Header>,
    >;
    type TransactionExecution: for<'a> TransactionExecutionStrategy<
        Block,
        Header,
        Database,
        Input<'a> = DbExecutionInput<'a, Block, Database>,
    >;
    type PostExecValidation: for<'a, 'b> PostExecutionValidationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = <Self::TransactionExecution as TransactionExecutionStrategy<
            Block,
            Header,
            Database,
        >>::Output<'a>,
        Output<'b> = BundleState,
    >;
    type Finalization: for<'a> FinalizationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = MPTFinalizationInput<'a, Block, Header>,
    >;

    fn validate(
        chain_spec: Arc<ChainSpec>,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<<Self::Finalization as FinalizationStrategy<Block, Header, Database>>::Output>
    {
        let mut engine =
            StatelessClientEngine::<Block, Header, Database>::new(chain_spec, data, None);
        engine.initialize_database::<Self::Initialization>()?;
        engine.pre_execution_validation::<Self::PreExecValidation>()?;
        let execution_output = engine.execute_transactions::<Self::TransactionExecution>()?;

        let bundle_state =
            StatelessClientEngine::<Block, Header, Database>::post_execution_validation::<
                Self::PostExecValidation,
            >(execution_output)?;

        engine.finalize::<Self::Finalization>(bundle_state)
    }
}

pub struct RethStatelessClient;

impl StatelessClient<Block, Header, InMemoryDB> for RethStatelessClient {
    type Initialization = InMemoryDbStrategy;
    type PreExecValidation = RethPreExecStrategy;
    type TransactionExecution = RethExecStrategy;
    type PostExecValidation = RethPostExecStrategy;
    type Finalization = RethFinalizationStrategy;
}
