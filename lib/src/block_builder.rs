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
use revm::{db::Database, DatabaseCommit};
use zeth_primitives::block::Header;

use crate::{
    consts::ChainSpec, derivation::HeaderDerivationStrategy, execution::TxExecStrategy,
    finalization::BlockBuildStrategy, initialization::DbInitStrategy, input::Input,
};

#[derive(Clone, Debug)]
pub struct BlockBuilder<'a, D> {
    pub(crate) chain_spec: &'a ChainSpec,
    pub(crate) input: Input,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<Header>,
}

impl<D> BlockBuilder<'_, D>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: core::fmt::Debug,
{
    /// Creates a new block builder.
    pub fn new(chain_spec: &ChainSpec, input: Input) -> BlockBuilder<'_, D> {
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

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }

    /// Initializes the database from the input tries.
    pub fn initialize_database<T: DbInitStrategy<Db = D>>(self) -> Result<Self> {
        T::initialize_database(self)
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn derive_header<T: HeaderDerivationStrategy>(self) -> Result<Self> {
        T::derive_header(self)
    }

    /// Executes the transactions.
    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Builds the block and returns the output.
    pub fn build<T: BlockBuildStrategy<Db = D>>(self) -> Result<T::Output> {
        T::build(self)
    }
}
