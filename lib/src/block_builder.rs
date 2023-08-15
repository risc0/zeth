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
use hashbrown::hash_map;
use revm::{
    db::DbAccount,
    primitives::{Account, Address, B160, U256},
};
use zeth_primitives::block::Header;

use crate::{
    consts::ChainSpec,
    execution::TxExecStrategy,
    finalization::BlockBuildStrategy,
    initialization::DbInitStrategy,
    validation::{
        compute_base_fee, compute_block_number, verify_extra_data, verify_gas_limit,
        verify_timestamp, Input,
    },
};

pub trait BlockBuilderDatabase: revm::Database + Sized {
    /// Returns all non-deleted accounts with their storage entries.
    fn accounts(&self) -> hash_map::Iter<B160, DbAccount>;
    /// Increases the balance of `address` by `amount`.
    fn increase_balance(&mut self, address: Address, amount: U256) -> Result<(), Self::Error>;
    /// Updates the account of `address`.
    fn update(&mut self, address: Address, account: Account);
}

#[derive(Clone, Debug)]
pub struct BlockBuilder<'a, D> {
    pub(crate) chain_spec: &'a ChainSpec,
    pub(crate) input: Input,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<Header>,
}

impl<D> BlockBuilder<'_, D>
where
    D: BlockBuilderDatabase,
    <D as revm::Database>::Error: std::fmt::Debug,
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

    /// Initializes the database from the input tries.
    pub fn initialize_database<T: DbInitStrategy<Db = D>>(mut self) -> Result<Self> {
        T::initialize_database(self)
    }

    /// Initializes the header. This must be called before executing transactions.
    pub fn initialize_header(mut self) -> Result<Self> {
        // Verify current block
        verify_gas_limit(self.input.gas_limit, self.input.parent_header.gas_limit)?;
        verify_timestamp(self.input.timestamp, self.input.parent_header.timestamp)?;
        verify_extra_data(&self.input.extra_data)?;
        // Initialize result header
        self.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: self.input.parent_header.hash(),
            number: compute_block_number(&self.input.parent_header)?,
            base_fee_per_gas: compute_base_fee(
                &self.input.parent_header,
                self.chain_spec.gas_constants(),
            )?,
            // Initialize metadata from input
            beneficiary: self.input.beneficiary,
            gas_limit: self.input.gas_limit,
            timestamp: self.input.timestamp,
            mix_hash: self.input.mix_hash,
            extra_data: self.input.extra_data.clone(),
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(self)
    }

    /// Executes the transactions.
    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Builds the block and returns the header.
    pub fn build<T: BlockBuildStrategy<Db = D>>(mut self, mut strategy: T) -> Result<Header> {
        strategy.build(self)
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
