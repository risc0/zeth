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

#[cfg(not(target_os = "zkvm"))]
use std::sync::{Arc, Mutex};

use anyhow::Result;
use revm::{primitives::SpecId, Database, DatabaseCommit};
use serde::Serialize;
use zeth_primitives::{
    block::Header,
    transactions::{
        ethereum::EthereumTxEssence, linea::LineaTxEssence, optimism::OptimismTxEssence, TxEssence,
    },
    trie::MptNode,
};

use crate::{
    builder::{
        execute::{ethereum::EthTxExecStrategy, optimism::OpTxExecStrategy, TxExecStrategy},
        finalize::{BlockFinalizeStrategy, MemDbBlockFinalizeStrategy},
        initialize::{DbInitStrategy, MemDbInitStrategy},
        prepare::{EthHeaderPrepStrategy, HeaderPrepStrategy},
    },
    consts::ChainSpec,
    input::BlockBuildInput,
    mem_db::MemDb,
    output::BlockBuildOutput,
};

mod execute;
mod finalize;
mod initialize;
mod prepare;

#[cfg(not(target_os = "zkvm"))]
type DatabaseRescue<D> = Arc<Mutex<Option<D>>>;
#[cfg(target_os = "zkvm")]
type DatabaseRescue<D> = core::marker::PhantomData<D>;

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct BlockBuilder<'a, D, E: TxEssence> {
    pub(crate) chain_spec: &'a ChainSpec,
    pub(crate) input: BlockBuildInput<E>,
    pub(crate) db: Option<D>,
    pub(crate) spec_id: Option<SpecId>,
    pub(crate) header: Option<Header>,
    pub db_drop_destination: Option<DatabaseRescue<D>>,
}

// This implementation allows us to recover data during erroneous block builds on the host
#[cfg(not(target_os = "zkvm"))]
impl<'a, D, E: TxEssence> Drop for BlockBuilder<'a, D, E> {
    fn drop(&mut self) {
        if let Some(backup_target) = &mut self.db_drop_destination {
            if let Some(dropped_db) = self.db.take() {
                if let Ok(mut target_option) = backup_target.lock() {
                    target_option.replace(dropped_db);
                }
            }
        }
    }
}

impl<D, E> BlockBuilder<'_, D, E>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
    E: TxEssence,
{
    /// Creates a new block builder.
    pub fn new(
        chain_spec: &ChainSpec,
        input: BlockBuildInput<E>,
        db_backup: Option<DatabaseRescue<D>>,
    ) -> BlockBuilder<'_, D, E> {
        BlockBuilder {
            chain_spec,
            db: None,
            spec_id: None,
            header: None,
            input,
            db_drop_destination: db_backup,
        }
    }

    /// Sets the database instead of initializing it from the input.
    pub fn with_db(mut self, db: D) -> Self {
        self.db = Some(db);
        self
    }

    /// Initializes the database from the input.
    pub fn initialize_database<T: DbInitStrategy<D>>(self) -> Result<Self> {
        T::initialize_database(self)
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header<T: HeaderPrepStrategy>(self) -> Result<Self> {
        T::prepare_header(self)
    }

    /// Executes all input transactions.
    pub fn execute_transactions<T: TxExecStrategy<E>>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Finalizes the block building and returns the header and the state trie.
    pub fn finalize<T: BlockFinalizeStrategy<D>>(self) -> Result<(Header, MptNode)> {
        T::finalize(self)
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }

    /// Destroys the builder and returns the database
    pub fn take_db(mut self) -> Option<D> {
        self.db.take()
    }
}

/// A bundle of strategies for building a block using [BlockBuilder].
pub trait BlockBuilderStrategy {
    type TxEssence: TxEssence + Serialize;

    type DbInitStrategy: DbInitStrategy<MemDb>;
    type HeaderPrepStrategy: HeaderPrepStrategy;
    type TxExecStrategy: TxExecStrategy<Self::TxEssence>;
    type BlockFinalizeStrategy: BlockFinalizeStrategy<MemDb>;

    /// Builds a block from the given input.
    fn build_from(
        chain_spec: &ChainSpec,
        input: BlockBuildInput<Self::TxEssence>,
    ) -> Result<BlockBuildOutput> {
        let input_hash = input.state_input.hash();

        let builder = BlockBuilder::<MemDb, Self::TxEssence>::new(chain_spec, input, None);

        // Database initialization errors do not indicate a faulty block
        let initialized = builder.initialize_database::<Self::DbInitStrategy>()?;

        // Recoverable header validation errors mean a faulty block
        let prepared = match initialized.prepare_header::<Self::HeaderPrepStrategy>() {
            Ok(builder) => builder,
            Err(_) => {
                return Ok(BlockBuildOutput::FAILURE {
                    state_input_hash: input_hash.into(),
                })
            }
        };

        // Recoverable transaction execution errors mean a faulty block
        let executed = match prepared.execute_transactions::<Self::TxExecStrategy>() {
            Ok(builder) => builder,
            Err(_) => {
                return Ok(BlockBuildOutput::FAILURE {
                    state_input_hash: input_hash.into(),
                })
            }
        };

        // Finalization errors do not indicate a faulty block
        let (header, state) = executed.finalize::<Self::BlockFinalizeStrategy>()?;

        Ok(BlockBuildOutput::SUCCESS {
            hash: header.hash(),
            head: header,
            state,
            state_input_hash: input_hash.into(),
        })
    }
}

/// The [BlockBuilderStrategy] for building an Ethereum block.
pub struct EthereumStrategy {}

impl BlockBuilderStrategy for EthereumStrategy {
    type TxEssence = EthereumTxEssence;
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = EthHeaderPrepStrategy;
    type TxExecStrategy = EthTxExecStrategy;
    type BlockFinalizeStrategy = MemDbBlockFinalizeStrategy;
}

/// The [BlockBuilderStrategy] for building an Optimism block.
pub struct OptimismStrategy {}

impl BlockBuilderStrategy for OptimismStrategy {
    type TxEssence = OptimismTxEssence;
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = EthHeaderPrepStrategy;
    type TxExecStrategy = OpTxExecStrategy;
    type BlockFinalizeStrategy = MemDbBlockFinalizeStrategy;
}

pub struct LineaStrategy {}

// impl BlockBuilderStrategy for LineaStrategy {
//     type TxEssence = LineaTxEssence,
//     type DbInitStrategy = todo!();
//     type HeaderPrepStrategy = todo!();
//     type TxExecStrategy = todo!();
//     type BlockFinalizeStrategy = todo!();
// }
