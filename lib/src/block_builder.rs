// Copyright 2023 RISC Zero, Inc.
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

use anyhow::Result;
use revm::{Database, DatabaseCommit};
use zeth_primitives::{
    block::Header,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence, TxEssence},
};

use crate::{
    consts::ChainSpec,
    execution::{ethereum::EthTxExecStrategy, optimism::OpTxExecStrategy, TxExecStrategy},
    finalization::{BlockBuildStrategy, BuildFromMemDbStrategy},
    initialization::{DbInitStrategy, MemDbInitStrategy},
    input::Input,
    mem_db::MemDb,
    preparation::{EthHeaderPrepStrategy, HeaderPrepStrategy},
};

#[derive(Clone, Debug)]
pub struct BlockBuilder<'a, D, E: TxEssence> {
    pub(crate) chain_spec: &'a ChainSpec,
    pub(crate) input: Input<E>,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<Header>,
}

impl<D, E> BlockBuilder<'_, D, E>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
    E: TxEssence,
{
    /// Creates a new block builder.
    pub fn new(chain_spec: &ChainSpec, input: Input<E>) -> BlockBuilder<'_, D, E> {
        BlockBuilder {
            chain_spec,
            db: None,
            header: None,
            input,
        }
    }

    /// Sets the database.
    pub fn with_db(mut self, db: D) -> Self {
        self.db = Some(db);
        self
    }

    /// Initializes the database from the input tries.
    pub fn initialize_database<T: DbInitStrategy<E, Database = D>>(self) -> Result<Self> {
        T::initialize_database(self)
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header<T: HeaderPrepStrategy>(self) -> Result<Self> {
        T::prepare_header(self)
    }

    /// Executes the transactions.
    pub fn execute_transactions<T: TxExecStrategy<E>>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Builds the block and returns the header.
    pub fn build<T: BlockBuildStrategy<E, Database = D>>(self) -> Result<T::Output> {
        T::build(self)
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }
}

pub trait NetworkStrategyBundle {
    type Database: Database + DatabaseCommit;
    type TxEssence: TxEssence;

    type DbInitStrategy: DbInitStrategy<Self::TxEssence, Database = Self::Database>;
    type HeaderPrepStrategy: HeaderPrepStrategy;
    type TxExecStrategy: TxExecStrategy<Self::TxEssence>;
    type BlockBuildStrategy: BlockBuildStrategy<Self::TxEssence, Database = Self::Database>;
}

pub struct ConfiguredBlockBuilder<'a, N: NetworkStrategyBundle>(
    BlockBuilder<'a, N::Database, N::TxEssence>,
)
where
    N::TxEssence: TxEssence;

impl<N: NetworkStrategyBundle> ConfiguredBlockBuilder<'_, N>
where
    <N::Database as Database>::Error: core::fmt::Debug,
{
    pub fn build_from(
        chain_spec: &ChainSpec,
        input: Input<N::TxEssence>,
    ) -> Result<<N::BlockBuildStrategy as BlockBuildStrategy<N::TxEssence>>::Output> {
        Self::new(chain_spec, input)
            .initialize_database()?
            .prepare_header()?
            .execute_transactions()?
            .build()
    }

    /// Creates a new block builder.
    pub fn new(
        chain_spec: &ChainSpec,
        input: Input<N::TxEssence>,
    ) -> ConfiguredBlockBuilder<'_, N> {
        ConfiguredBlockBuilder(BlockBuilder::new(chain_spec, input))
    }

    /// Sets the database.
    pub fn with_db(mut self, db: N::Database) -> Self {
        self.0.db = Some(db);
        self
    }

    /// Initializes the database from the input tries.
    pub fn initialize_database(self) -> Result<Self> {
        Ok(ConfiguredBlockBuilder(
            N::DbInitStrategy::initialize_database(self.0)?,
        ))
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header(self) -> Result<Self> {
        Ok(ConfiguredBlockBuilder(
            N::HeaderPrepStrategy::prepare_header(self.0)?,
        ))
    }

    /// Executes the transactions.
    pub fn execute_transactions(self) -> Result<Self> {
        Ok(ConfiguredBlockBuilder(
            N::TxExecStrategy::execute_transactions(self.0)?,
        ))
    }

    /// Builds the block and returns the header.
    pub fn build(
        self,
    ) -> Result<<N::BlockBuildStrategy as BlockBuildStrategy<N::TxEssence>>::Output> {
        N::BlockBuildStrategy::build(self.0)
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&N::Database> {
        self.0.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut N::Database> {
        self.0.db.as_mut()
    }
}

pub struct EthereumStrategyBundle {}

impl NetworkStrategyBundle for EthereumStrategyBundle {
    type Database = MemDb;
    type TxEssence = EthereumTxEssence;
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = EthHeaderPrepStrategy;
    type TxExecStrategy = EthTxExecStrategy;
    type BlockBuildStrategy = BuildFromMemDbStrategy;
}

pub type EthereumBlockBuilder<'a> = ConfiguredBlockBuilder<'a, EthereumStrategyBundle>;

pub struct OptimismStrategyBundle {}

impl NetworkStrategyBundle for OptimismStrategyBundle {
    type Database = MemDb;
    type TxEssence = OptimismTxEssence;
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = EthHeaderPrepStrategy;
    type TxExecStrategy = OpTxExecStrategy;
    type BlockBuildStrategy = BuildFromMemDbStrategy;
}

pub type OptimismBlockBuilder<'a> = ConfiguredBlockBuilder<'a, OptimismStrategyBundle>;
