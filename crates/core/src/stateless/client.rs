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

use crate::rescue::{Recoverable, Wrapper};
use crate::stateless::data::StatelessClientData;
use crate::stateless::driver::SCEDriver;
use crate::stateless::engine::StatelessClientEngine;
use crate::stateless::execute::{DbExecutionInput, TransactionExecutionStrategy};
use crate::stateless::finalize::{FinalizationStrategy, MPTFinalizationInput};
use crate::stateless::initialize::{InitializationStrategy, MPTInitializationInput};
use crate::stateless::post_exec::PostExecutionValidationStrategy;
use crate::stateless::pre_exec::{ConsensusPreExecValidationInput, PreExecutionValidationStrategy};
use reth_evm_ethereum::execute::EthBatchExecutor;
use reth_evm_ethereum::EthEvmConfig;
use reth_revm::db::BundleState;
use serde::de::DeserializeOwned;
use std::io::Read;
use std::sync::{Arc, Mutex};

pub type RescueDestination<D> = Arc<Mutex<Option<D>>>;

pub trait StatelessClient<ChainSpec, Block, Header, Database, Driver>
where
    Block: DeserializeOwned + 'static,
    Header: DeserializeOwned + 'static,
    Database: Recoverable + 'static,
    Driver: SCEDriver<Block, Header> + 'static,
{
    type Initialization: for<'a, 'b> InitializationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = MPTInitializationInput<'a, Header>,
        Output<'b> = Database,
    >;
    type PreExecValidation: for<'a> PreExecutionValidationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = ConsensusPreExecValidationInput<'a, ChainSpec, Block, Header>,
    >;
    type TransactionExecution: for<'a, 'b> TransactionExecutionStrategy<
        Block,
        Header,
        Wrapper<Database>,
        Input<'a> = DbExecutionInput<'a, ChainSpec, Block, Wrapper<Database>>,
        Output<'b> = EthBatchExecutor<EthEvmConfig, Wrapper<Database>>,
    >;
    type PostExecValidation: for<'a, 'b> PostExecutionValidationStrategy<
        Block,
        Header,
        Wrapper<Database>,
        Input<'a> = <Self::TransactionExecution as TransactionExecutionStrategy<
            Block,
            Header,
            Wrapper<Database>,
        >>::Output<'a>,
        Output<'b> = BundleState,
    >;
    type Finalization: for<'a> FinalizationStrategy<
        Block,
        Header,
        Database,
        Input<'a> = MPTFinalizationInput<'a, Block, Header, Database>,
    >;

    fn deserialize_data<I: Read>(reader: I) -> anyhow::Result<StatelessClientData<Block, Header>> {
        Ok(pot::from_reader(reader)?)
    }

    fn validate(
        chain_spec: Arc<ChainSpec>,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<StatelessClientEngine<ChainSpec, Block, Header, Database, Driver>> {
        // Instantiate the engine and initialize the database
        let mut engine = StatelessClientEngine::<ChainSpec, Block, Header, Database, Driver>::new(
            chain_spec, data, None,
        );
        engine.initialize_database::<Self::Initialization>()?;
        // Run the engine until all blocks are processed
        while !engine.data.blocks.is_empty() {
            engine.pre_execution_validation::<Self::PreExecValidation>()?;
            let execution_output = engine.execute_transactions::<Self::TransactionExecution>()?;
            let bundle_state =
                engine.post_execution_validation::<Self::PostExecValidation>(execution_output)?;
            // Skip the database update if we're finalizing the last block
            if engine.data.blocks.len() == 1 {
                engine.db.take();
            }
            engine.finalize::<Self::Finalization>(bundle_state)?;
        }
        // Return the engine for inspection
        Ok(engine)
    }
}
