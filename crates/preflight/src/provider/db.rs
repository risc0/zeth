// Copyright 2023, 2024 RISC Zero, Inc.
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

use crate::driver::PreflightDriver;
use crate::provider::query::{AccountQuery, BlockQuery, StorageQuery};
use crate::provider::Provider;
use alloy::network::Network;
use alloy::primitives::map::HashMap;
use alloy::primitives::{Address, B256, U256};
use reth_revm::bytecode::Bytecode;
use reth_revm::state::{Account, AccountInfo};
use reth_revm::{Database, DatabaseCommit};
use reth_storage_errors::db::DatabaseError;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use zeth_core::driver::CoreDriver;

pub struct ProviderDB<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> {
    pub provider: Rc<RefCell<dyn Provider<N>>>,
    pub block_no: u64,
    /// Bytecode cache to allow querying bytecode by hash instead of address.
    pub contracts: HashMap<B256, Bytecode>,

    pub driver: PhantomData<(R, P)>,
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> Clone for ProviderDB<N, R, P> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            block_no: self.block_no,
            contracts: self.contracts.clone(),
            driver: self.driver,
        }
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> ProviderDB<N, R, P> {
    pub fn new(provider: Rc<RefCell<dyn Provider<N>>>, block_no: u64) -> Self {
        ProviderDB {
            provider,
            block_no,
            contracts: HashMap::default(),
            driver: PhantomData,
        }
    }

    pub fn advance_provider_block(&mut self) -> anyhow::Result<()> {
        self.block_no += 1;
        self.provider.borrow_mut().advance()
    }

    pub fn save_provider(&self) -> anyhow::Result<()> {
        self.provider.borrow().save()
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> Database for ProviderDB<N, R, P> {
    type Error = DatabaseError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let query = AccountQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
        };
        let nonce = self
            .provider
            .borrow_mut()
            .get_transaction_count(&query)
            .map_err(db_error)?;
        let balance = self
            .provider
            .borrow_mut()
            .get_balance(&query)
            .map_err(db_error)?;
        let code = self
            .provider
            .borrow_mut()
            .get_code(&query)
            .map_err(db_error)?;
        let bytecode = Bytecode::new_raw(code);

        // if the account is empty return None
        // in the EVM, emptiness is treated as equivalent to nonexistence
        if nonce.is_zero() && balance.is_zero() && bytecode.is_empty() {
            return Ok(None);
        }

        // index the code by its hash, so that we can later use code_by_hash
        let code_hash = bytecode.hash_slow();
        self.contracts.insert(code_hash, bytecode);

        Ok(Some(AccountInfo {
            nonce: nonce.to(),
            balance,
            code_hash,
            code: None, // will be queried later using code_by_hash
        }))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        // this works because `basic` is always called first
        let code = self
            .contracts
            .get(&code_hash)
            .expect("`basic` must be called first for the corresponding account");

        Ok(code.clone())
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let bytes = index.to_be_bytes::<32>();
        let index = U256::from_be_bytes(bytes);

        self.provider
            .borrow_mut()
            .get_storage(&StorageQuery {
                block_no: self.block_no,
                address: address.into_array().into(),
                index,
            })
            .map_err(db_error)
    }

    fn block_hash(&mut self, block_no: u64) -> Result<B256, Self::Error> {
        let header = P::derive_header(P::derive_header_response(
            self.provider
                .borrow_mut()
                .get_full_block(&BlockQuery { block_no })
                .map_err(db_error)?,
        ));
        Ok(R::header_hash(&header))
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> DatabaseCommit for ProviderDB<N, R, P> {
    fn commit(&mut self, _changes: HashMap<Address, Account>) {}
}

fn db_error(err: anyhow::Error) -> DatabaseError {
    DatabaseError::Other(format!("provider error: {err:#}"))
}
