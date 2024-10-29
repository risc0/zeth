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

use crate::provider::{AccountQuery, BlockQuery, Provider, StorageQuery};
use alloy::primitives::map::HashMap;
use alloy::primitives::{Address, B256, U256};
use reth_revm::primitives::{Account, AccountInfo, Bytecode};
use reth_revm::{Database, DatabaseCommit, DatabaseRef};
use reth_storage_errors::db::DatabaseError;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct ProviderDB {
    pub provider: Rc<RefCell<dyn Provider>>,
    pub block_no: u64,
}

impl ProviderDB {
    pub fn new(provider: Rc<RefCell<dyn Provider>>, block_no: u64) -> Self {
        ProviderDB { provider, block_no }
    }

    pub fn advance_provider_block(&mut self) -> anyhow::Result<()> {
        self.block_no += 1;
        self.provider.borrow_mut().advance()
    }

    pub fn save_provider(&self) -> anyhow::Result<()> {
        self.provider.borrow().save()
    }
}

impl Database for ProviderDB {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let query = AccountQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
        };
        let nonce = self.provider.borrow_mut().get_transaction_count(&query)?;
        let balance = self.provider.borrow_mut().get_balance(&query)?;
        let code = self.provider.borrow_mut().get_code(&query)?;
        let bytecode = Bytecode::new_raw(code);
        Ok(Some(AccountInfo::new(
            balance,
            nonce.to(),
            bytecode.hash_slow(),
            bytecode,
        )))
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let bytes = index.to_be_bytes::<32>();
        let index = U256::from_be_bytes(bytes);

        self.provider.borrow_mut().get_storage(&StorageQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
            index,
        })
    }

    fn block_hash(&mut self, block_no: u64) -> Result<B256, Self::Error> {
        Ok(self
            .provider
            .borrow_mut()
            .get_full_block(&BlockQuery { block_no })?
            .header
            .hash)
    }
}

impl DatabaseRef for ProviderDB {
    type Error = DatabaseError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let query = AccountQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
        };
        let nonce = self
            .provider
            .borrow_mut()
            .get_transaction_count(&query)
            .unwrap();
        let balance = self.provider.borrow_mut().get_balance(&query).unwrap();
        let code = self.provider.borrow_mut().get_code(&query).unwrap();
        let bytecode = Bytecode::new_raw(code);
        Ok(Some(AccountInfo::new(
            balance,
            nonce.to(),
            bytecode.hash_slow(),
            bytecode,
        )))
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!("code_by_hash_ref")
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let bytes = index.to_be_bytes::<32>();
        let index = U256::from_be_bytes(bytes);

        Ok(self
            .provider
            .borrow_mut()
            .get_storage(&StorageQuery {
                block_no: self.block_no,
                address: address.into_array().into(),
                index,
            })
            .unwrap())
    }

    fn block_hash_ref(&self, block_no: u64) -> Result<B256, Self::Error> {
        Ok(self
            .provider
            .borrow_mut()
            .get_full_block(&BlockQuery { block_no })
            .unwrap()
            .header
            .hash)
    }
}

impl DatabaseCommit for ProviderDB {
    fn commit(&mut self, _changes: HashMap<Address, Account>) {}
}
