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

use alloy_consensus::Header as AlloyConsensusHeader;
use anyhow::Result;
use revm::{Database, DatabaseCommit};
use zeth_primitives::mpt::MptNode;

pub use self::execute::TkoTxExecStrategy;
use crate::{
    builder::{
        finalize::{BlockFinalizeStrategy, MemDbBlockFinalizeStrategy},
        initialize::{DbInitStrategy, MemDbInitStrategy},
        prepare::{HeaderPrepStrategy, TaikoHeaderPrepStrategy},
    },
    consts::{get_chain_spec, ChainSpec},
    input::GuestInput,
    mem_db::MemDb,
};

pub mod execute;
mod finalize;
mod initialize;
pub mod prepare;

/// A generic builder for building a block.
#[derive(Clone, Debug)]
pub struct BlockBuilder<D> {
    pub(crate) chain_spec: ChainSpec,
    pub(crate) input: GuestInput,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<AlloyConsensusHeader>,
}

impl<D> BlockBuilder<D>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
{
    /// Creates a new block builder.
    pub fn new(input: &GuestInput) -> BlockBuilder<D> {
        BlockBuilder {
            chain_spec: get_chain_spec(&input.taiko.chain_spec_name),
            db: None,
            header: None,
            input: input.clone(),
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
    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Finalizes the block building and returns the header and the state trie.
    pub fn finalize<T: BlockFinalizeStrategy<D>>(self) -> Result<(AlloyConsensusHeader, MptNode)> {
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
}

/// A bundle of strategies for building a block using [BlockBuilder].
pub trait BlockBuilderStrategy {
    type DbInitStrategy: DbInitStrategy<MemDb>;
    type HeaderPrepStrategy: HeaderPrepStrategy;
    type TxExecStrategy: TxExecStrategy;
    type BlockFinalizeStrategy: BlockFinalizeStrategy<MemDb>;

    /// Builds a block from the given input.
    fn build_from(input: &GuestInput) -> Result<(AlloyConsensusHeader, MptNode)> {
        BlockBuilder::<MemDb>::new(input)
            .initialize_database::<Self::DbInitStrategy>()?
            .prepare_header::<Self::HeaderPrepStrategy>()?
            .execute_transactions::<Self::TxExecStrategy>()?
            .finalize::<Self::BlockFinalizeStrategy>()
    }
}

/// The [BlockBuilderStrategy] for building a Taiko block.
pub struct TaikoStrategy {}
impl BlockBuilderStrategy for TaikoStrategy {
    type DbInitStrategy = MemDbInitStrategy;
    type HeaderPrepStrategy = TaikoHeaderPrepStrategy;
    type TxExecStrategy = TkoTxExecStrategy;
    type BlockFinalizeStrategy = MemDbBlockFinalizeStrategy;
}
pub trait TxExecStrategy {
    fn execute_transactions<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: core::fmt::Debug;
}
