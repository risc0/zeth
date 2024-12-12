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
use crate::stateless::data::{
    RkyvStatelessClientData, StatelessClientChainData, StatelessClientData,
};
use crate::stateless::engine::StatelessClientEngine;
use crate::stateless::execute::ExecutionStrategy;
use crate::stateless::finalize::FinalizationStrategy;
use crate::stateless::initialize::InitializationStrategy;
use crate::stateless::validate::ValidationStrategy;

pub trait StatelessClient<Driver, Database>
where
    Driver: CoreDriver + 'static,
    Database: Recoverable + 'static,
{
    type Initialization: InitializationStrategy<Driver, Database>;
    type Validation: ValidationStrategy<Driver, Database>;
    type Execution: ExecutionStrategy<Driver, Wrapper<Database>>;
    type Finalization: FinalizationStrategy<Driver, Database>;

    fn data_from_parts(
        rkyv_slice: &[u8],
        pot_slice: &[u8],
    ) -> anyhow::Result<StatelessClientData<Driver::Block, Driver::Header>> {
        let rkyv_data =
            rkyv::from_bytes::<RkyvStatelessClientData, rkyv::rancor::Error>(rkyv_slice)?;
        let chain_data =
            pot::from_slice::<StatelessClientChainData<Driver::Block, Driver::Header>>(pot_slice)?;
        Ok(StatelessClientData::<Driver::Block, Driver::Header>::from_parts(rkyv_data, chain_data))
    }

    fn data_from_slice(
        slice: &[u8],
    ) -> anyhow::Result<StatelessClientData<Driver::Block, Driver::Header>> {
        Ok(pot::from_slice(slice)?)
    }

    fn validate(
        data: StatelessClientData<Driver::Block, Driver::Header>,
    ) -> anyhow::Result<StatelessClientEngine<Driver, Database>> {
        // Instantiate the engine and initialize the database
        let mut engine = StatelessClientEngine::<Driver, Database>::new(data, None);
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
