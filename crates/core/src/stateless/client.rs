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
use crate::stateless::driver::{RethDriver, SCEDriver};
use crate::stateless::engine::StatelessClientEngine;
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
use reth_chainspec::ChainSpec;
use reth_primitives::Block;
use reth_revm::db::BundleState;
use reth_revm::InMemoryDB;
use std::sync::{Arc, Mutex};

pub type RescueDestination<D> = Arc<Mutex<Option<D>>>;

pub trait StatelessClient<Block, Header, Database, Driver>
where
    Block: 'static,
    Header: 'static,
    Database: 'static,
    Driver: SCEDriver<Block, Header> + 'static,
{
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
            StatelessClientEngine::<Block, Header, Database, Driver>::new(chain_spec, data, None);
        engine.initialize_database::<Self::Initialization>()?;
        engine.pre_execution_validation::<Self::PreExecValidation>()?;
        let execution_output = engine.execute_transactions::<Self::TransactionExecution>()?;

        let bundle_state =
            StatelessClientEngine::<Block, Header, Database, Driver>::post_execution_validation::<
                Self::PostExecValidation,
            >(execution_output)?;

        engine.finalize::<Self::Finalization>(bundle_state)
    }
}

pub struct RethStatelessClient;

impl StatelessClient<Block, Header, InMemoryDB, RethDriver> for RethStatelessClient {
    type Initialization = InMemoryDbStrategy;
    type PreExecValidation = RethPreExecStrategy;
    type TransactionExecution = RethExecStrategy;
    type PostExecValidation = RethPostExecStrategy;
    type Finalization = RethFinalizationStrategy;
}
