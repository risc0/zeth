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

use crate::driver::CoreDriver;
use crate::rescue::{Recoverable, Wrapper};
use crate::stateless::data::StatelessClientData;
use crate::stateless::engine::StatelessClientEngine;
use crate::stateless::execute::ExecutionStrategy;
use crate::stateless::finalize::FinalizationStrategy;
use crate::stateless::initialize::InitializationStrategy;
use crate::stateless::validate::ValidationStrategy;
use std::io::Read;
use std::sync::{Arc, Mutex};

pub type RescueDestination<D> = Arc<Mutex<Option<D>>>;

pub trait StatelessClient<ChainSpec, Driver, Database>
where
    Driver: CoreDriver + 'static,
    Database: Recoverable + 'static,
{
    type Initialization: for<'a, 'b> InitializationStrategy<Driver, Database>;
    type Validation: for<'a> ValidationStrategy<ChainSpec, Driver, Database>;
    type Execution: for<'a, 'b> ExecutionStrategy<ChainSpec, Driver, Wrapper<Database>>;
    type Finalization: for<'a> FinalizationStrategy<Driver, Database>;

    fn deserialize_data<I: Read>(
        reader: I,
    ) -> anyhow::Result<StatelessClientData<Driver::Block, Driver::Header>> {
        Ok(pot::from_reader(reader)?)
    }

    fn validate(
        chain_spec: Arc<ChainSpec>,
        data: StatelessClientData<Driver::Block, Driver::Header>,
    ) -> anyhow::Result<StatelessClientEngine<ChainSpec, Driver, Database>> {
        // Instantiate the engine and initialize the database
        let mut engine =
            StatelessClientEngine::<ChainSpec, Driver, Database>::new(chain_spec, data, None);
        engine.initialize_database::<Self::Initialization>()?;
        // Run the engine until all blocks are processed
        while !engine.data.blocks.is_empty() {
            engine.validate_header::<Self::Validation>()?;
            let bundle_state = engine.execute_transactions::<Self::Execution>()?;
            // Skip the database update if we're finalizing the last block
            if engine.data.blocks.len() == 1 {
                engine.db.take();
            }
            engine.finalize_state::<Self::Finalization>(bundle_state)?;
        }
        // Return the engine for inspection
        Ok(engine)
    }
}
