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

use crate::stateless::block::StatelessClientBlock;
use crate::stateless::execute::TransactionExecutionStrategy;
use crate::stateless::initialize::InitializationStrategy;
use crate::stateless::post_exec::PostExecutionValidationStrategy;
use crate::stateless::pre_exec::PreExecutionValidationStrategy;
use alloy_primitives::U256;
use reth_chainspec::ChainSpec;
use std::sync::{Arc, Mutex};
// use crate::stateless::finalize::FinalizationStrategy;

type RescueDestination<D> = Arc<Mutex<Option<D>>>;

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct StatelessClientEngine<Block, Header, Database> {
    pub chain_spec: Arc<ChainSpec>,
    pub block: StatelessClientBlock<Block, Header>,
    pub total_difficulty: U256,
    pub db: Option<Database>,
    pub db_rescue: Option<RescueDestination<Database>>,
}

// This implementation allows us to recover data during erroneous block builds
impl<Block, Header, Database> Drop for StatelessClientEngine<Block, Header, Database> {
    fn drop(&mut self) {
        if let Some(backup_target) = &mut self.db_rescue {
            if let Some(dropped_db) = self.db.take() {
                if let Ok(mut target_option) = backup_target.lock() {
                    target_option.replace(dropped_db);
                }
            }
        }
    }
}

impl<Block, Header, Database> StatelessClientEngine<Block, Header, Database> {
    /// Creates a new stateless validator
    pub fn new(
        chain_spec: Arc<ChainSpec>,
        block: StatelessClientBlock<Block, Header>,
        total_difficulty: U256,
        db: Option<Database>,
        db_rescue: Option<RescueDestination<Database>>,
    ) -> Self {
        Self {
            chain_spec,
            block,
            total_difficulty,
            db,
            db_rescue,
        }
    }

    /// Initializes the database from the input.
    pub fn initialize_database<T: InitializationStrategy<Block, Header, Database>>(
        &mut self,
    ) -> anyhow::Result<T::Output> {
        T::initialize_database(self)
    }

    /// Validates the header before execution.
    pub fn pre_execution_validation<T: PreExecutionValidationStrategy<Block, Header, Database>>(
        &mut self,
    ) -> anyhow::Result<T::Output> {
        T::pre_execution_validation(self)
    }

    /// Executes transactions.
    pub fn execute_transactions<T: TransactionExecutionStrategy<Block, Header, Database>>(
        &mut self,
    ) -> anyhow::Result<T::Output> {
        T::execute_transactions(self)
    }

    /// Validates the header after execution.
    pub fn post_execution_validation<
        T1: TransactionExecutionStrategy<Block, Header, Database>,
        T2: PostExecutionValidationStrategy<Block, Header, Database, T1>,
    >(
        &mut self,
        execution_output: T1::Output,
    ) -> anyhow::Result<T2::Output> {
        T2::post_execution_validation(self, execution_output)
    }
    //
    // /// Finalizes the state trie.
    // pub fn finalize<
    //     T: FinalizationStrategy<Block, Header, Database, U>,
    //     U
    // >(self) -> anyhow::Result<T::Output> {
    //     T::finalize(self)
    // }
}

pub trait StatelessClientStrategy<Block, Header, Database> {
    type Initialization: InitializationStrategy<Block, Header, Database>;
    type PreExecValidation: PreExecutionValidationStrategy<Block, Header, Database>;
    type TransactionExecution: TransactionExecutionStrategy<Block, Header, Database>;
    type PostExecValidation: PostExecutionValidationStrategy<
        Block,
        Header,
        Database,
        Self::TransactionExecution,
    >;
    // type Finalization: FinalizationStrategy<Block, Header, Database, Self::PostExecValidation>;

    fn validate_block(
        chain_spec: Arc<ChainSpec>,
        block: StatelessClientBlock<Block, Header>,
        total_difficulty: U256,
    ) -> anyhow::Result<()> {
        let mut engine = StatelessClientEngine::<Block, Header, Database>::new(
            chain_spec,
            block,
            total_difficulty,
            None,
            None,
        );
        engine.initialize_database::<Self::Initialization>()?;
        engine.pre_execution_validation::<Self::PreExecValidation>()?;
        let execution_output = engine.execute_transactions::<Self::TransactionExecution>()?;

        // todo: when testing this function, implement tests at each fork that mess with the
        // intermediate inputs/outputs to check whether all header fields (e.g. receipts/txn/state trie roots) are
        // properly validated

        let state_delta = engine
            .post_execution_validation::<Self::TransactionExecution, Self::PostExecValidation>(
                execution_output,
            )?;

        // engine.finalize::<Self::Finalization, Self::PostExecValidation>()?;
        Ok(())
    }
}
