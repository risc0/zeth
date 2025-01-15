// Copyright 2024, 2025 RISC Zero, Inc.
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
use crate::rescue::{Recoverable, Rescued, Wrapper};
use crate::stateless::data::StatelessClientData;
use crate::stateless::execute::ExecutionStrategy;
use crate::stateless::finalize::FinalizationStrategy;
use crate::stateless::initialize::InitializationStrategy;
use crate::stateless::validate::ValidationStrategy;
use anyhow::Context;
use reth_revm::db::BundleState;

/// A generic builder for building a block.
pub struct StatelessClientEngine<Driver: CoreDriver, Database: Recoverable> {
    pub data: StatelessClientData<Driver::Block, Driver::Header>,
    pub db: Option<Wrapper<Database>>,
    pub db_rescued: Option<Rescued<Database>>,
}

impl<Driver: CoreDriver, Database: Recoverable> StatelessClientEngine<Driver, Database> {
    /// Creates a new stateless validator
    pub fn new(
        data: StatelessClientData<Driver::Block, Driver::Header>,
        db: Option<Database>,
    ) -> Self {
        let db = db.map(|db| Wrapper::from(db));
        let db_rescued = db.as_ref().map(|db| db.rescued());
        Self {
            data,
            db,
            db_rescued,
        }
    }

    /// Initializes the database from the input.
    pub fn initialize_database<T: InitializationStrategy<Driver, Database>>(
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
        let new_db = Wrapper::from(
            T::initialize_database(
                state_trie,
                storage_tries,
                contracts,
                parent_header,
                ancestor_headers,
            )
            .context("StatelessClientEngine::initialize_database")?,
        );
        self.db_rescued = Some(new_db.rescued());
        Ok(db
            .replace(new_db)
            .and_then(|mut rescue_db| rescue_db.rescue()))
    }

    /// Validates the header before execution.
    pub fn validate_header<T: ValidationStrategy<Driver, Database>>(
        &mut self,
    ) -> anyhow::Result<()> {
        // Unpack input
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    chain,
                    blocks,
                    parent_header,
                    total_difficulty,
                    ..
                },
            ..
        } = self;
        T::validate_header(
            Driver::chain_spec(chain).unwrap(),
            blocks.last_mut().unwrap(),
            parent_header,
            total_difficulty,
        )
        .context("StatelessClientEngine::validate_header")
    }

    /// Executes transactions.
    pub fn execute_transactions<T: ExecutionStrategy<Driver, Wrapper<Database>>>(
        &mut self,
    ) -> anyhow::Result<BundleState> {
        // Unpack input
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    chain,
                    blocks,
                    signers,
                    total_difficulty,
                    ..
                },
            db,
            ..
        } = self;
        // Execute transactions
        let bundle_state = T::execute_transactions(
            Driver::chain_spec(chain).unwrap(),
            blocks.last_mut().unwrap(),
            signers.last().unwrap(),
            total_difficulty,
            db,
        )
        .context("StatelessClientEngine::execute_transactions")?;
        // Rescue database
        if let Some(rescued) = self.db_rescued.take() {
            self.replace_db(Wrapper::from(rescued))?;
        }
        Ok(bundle_state)
    }

    pub fn replace_db(
        &mut self,
        new_db: Wrapper<Database>,
    ) -> anyhow::Result<Option<Wrapper<Database>>> {
        self.db_rescued.replace(new_db.rescued());
        Ok(self.db.replace(new_db))
    }

    /// Finalizes the state trie.
    pub fn finalize_state<T: FinalizationStrategy<Driver, Database>>(
        &mut self,
        bundle_state: BundleState,
        with_further_updates: bool,
    ) -> anyhow::Result<()> {
        // Unpack input
        let StatelessClientEngine {
            data:
                StatelessClientData {
                    blocks,
                    signers,
                    state_trie,
                    storage_tries,
                    parent_header,
                    total_difficulty,
                    ..
                },
            db,
            ..
        } = self;
        let db = db.as_mut();
        // Follow finalization strategy
        T::finalize_state(
            blocks.last_mut().unwrap(),
            state_trie,
            storage_tries,
            parent_header,
            db.map(|db| &mut db.inner),
            bundle_state,
            with_further_updates,
        )
        .context("StatelessClientEngine::finalize")?;
        // Prepare for next block
        *parent_header = Driver::block_to_header(blocks.pop().unwrap());
        signers.pop();
        *total_difficulty = Driver::accumulate_difficulty(*total_difficulty, &*parent_header);

        Ok(())
    }
}
