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

use std::collections::BTreeSet;

use ethers_core::types::{EIP1186ProofResponse, H160, H256};
use hashbrown::{hash_map, HashMap};
use revm::{
    primitives::{Account, AccountInfo, Bytecode, B160, B256, U256},
    Database,
};
use zeth_primitives::block::Header;

use crate::{
    block_builder::BlockBuilderDatabase,
    host::provider::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery},
    mem_db::{DbAccount, DbError, MemDb},
};

pub struct ProviderDb {
    provider: Box<dyn Provider>,
    block_no: u64,
    initial_db: MemDb,
    latest_db: MemDb,
}

impl ProviderDb {
    pub fn new(provider: Box<dyn Provider>, block_no: u64) -> Self {
        ProviderDb {
            provider,
            block_no,
            initial_db: MemDb::default(),
            latest_db: MemDb::default(),
        }
    }

    pub fn get_provider(&self) -> &dyn Provider {
        self.provider.as_ref()
    }

    pub fn get_initial_db(&self) -> &MemDb {
        &self.initial_db
    }

    pub fn get_latest_db(&self) -> &MemDb {
        &self.latest_db
    }

    fn get_proofs(
        &mut self,
        block_no: u64,
        storage_keys: HashMap<B160, Vec<U256>>,
    ) -> Result<HashMap<B160, EIP1186ProofResponse>, anyhow::Error> {
        let mut out = HashMap::new();

        for (address, indices) in storage_keys {
            let proof = {
                let address: H160 = address.into();
                let indices: BTreeSet<H256> = indices
                    .into_iter()
                    .map(|x| x.to_be_bytes().into())
                    .collect();
                self.provider.get_proof(&ProofQuery {
                    block_no,
                    address,
                    indices,
                })?
            };
            out.insert(address, proof);
        }

        Ok(out)
    }

    pub fn get_initial_proofs(
        &mut self,
    ) -> Result<HashMap<B160, EIP1186ProofResponse>, anyhow::Error> {
        self.get_proofs(self.block_no, self.initial_db.storage_keys())
    }

    pub fn get_latest_proofs(
        &mut self,
    ) -> Result<HashMap<B160, EIP1186ProofResponse>, anyhow::Error> {
        let mut storage_keys = self.initial_db.storage_keys();

        for (address, mut indices) in self.latest_db.storage_keys() {
            match storage_keys.get_mut(&address) {
                Some(initial_indices) => initial_indices.append(&mut indices),
                None => {
                    storage_keys.insert(address, indices);
                }
            }
        }

        self.get_proofs(self.block_no + 1, storage_keys)
    }

    pub fn get_ancestor_headers(&mut self) -> Result<Vec<Header>, anyhow::Error> {
        let earliest_block = self
            .initial_db
            .block_hashes
            .keys()
            .min()
            .unwrap_or(&self.block_no);
        let headers = (*earliest_block..self.block_no)
            .rev()
            .map(|block_no| {
                self.provider
                    .get_partial_block(&BlockQuery { block_no })
                    .expect("Failed to retrieve ancestor block")
                    .try_into()
                    .expect("Failed to convert ethers block to zeth block")
            })
            .collect();
        Ok(headers)
    }
}

impl Database for ProviderDb {
    type Error = anyhow::Error;

    fn basic(&mut self, address: B160) -> Result<Option<AccountInfo>, Self::Error> {
        match self.latest_db.basic(address) {
            Ok(db_result) => return Ok(db_result),
            Err(DbError::AccountNotFound(_)) => {}
            Err(err) => return Err(err.into()),
        }
        match self.initial_db.basic(address) {
            Ok(db_result) => return Ok(db_result),
            Err(DbError::AccountNotFound(_)) => {}
            Err(err) => return Err(err.into()),
        }

        let account_info = {
            let address = H160::from(address.0);
            let query = AccountQuery {
                block_no: self.block_no,
                address,
            };
            let nonce = self.provider.get_transaction_count(&query)?;
            let balance = self.provider.get_balance(&query)?;
            let code = self.provider.get_code(&query)?;

            AccountInfo::new(balance.into(), nonce.as_u64(), Bytecode::new_raw(code.0))
        };

        self.initial_db
            .insert_account_info(address, account_info.clone());
        Ok(Some(account_info))
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    fn storage(&mut self, address: B160, index: U256) -> Result<U256, Self::Error> {
        match self.latest_db.storage(address, index) {
            Ok(db_result) => return Ok(db_result),
            Err(DbError::AccountNotFound(_)) | Err(DbError::SlotNotFound(_, _)) => {}
            Err(err) => return Err(err.into()),
        }
        match self.initial_db.storage(address, index) {
            Ok(db_result) => return Ok(db_result),
            Err(DbError::AccountNotFound(_)) | Err(DbError::SlotNotFound(_, _)) => {}
            Err(err) => return Err(err.into()),
        }

        // ensure that the corresponding account is loaded
        self.initial_db.basic(address)?;

        let storage = {
            let address = H160::from(address.0);
            let bytes = index.to_be_bytes();
            let index = H256::from(bytes);

            let storage = self.provider.get_storage(&StorageQuery {
                block_no: self.block_no,
                address,
                index,
            })?;
            ethers_core::types::U256::from(storage.0)
        };

        self.initial_db
            .insert_account_storage(&address, index, storage.into());
        Ok(storage.into())
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        match self.initial_db.block_hash(number) {
            Ok(block_hash) => return Ok(block_hash),
            Err(DbError::BlockNotFound(_)) => {}
            Err(err) => return Err(err.into()),
        }

        let block_no = u64::try_from(number).unwrap();
        let block_hash = self
            .provider
            .get_partial_block(&BlockQuery { block_no })?
            .hash
            .unwrap()
            .0
            .into();

        self.initial_db.insert_block_hash(block_no, block_hash);
        Ok(block_hash)
    }
}

impl BlockBuilderDatabase for ProviderDb {
    fn load(_accounts: HashMap<B160, DbAccount>, _block_hashes: HashMap<u64, B256>) -> Self {
        unimplemented!()
    }

    fn accounts(&self) -> hash_map::Iter<B160, DbAccount> {
        self.latest_db.accounts()
    }

    fn increase_balance(&mut self, address: B160, amount: U256) -> Result<(), Self::Error> {
        // ensure that the address is loaded into the latest_db
        if let Some(account_info) = self.basic(address)? {
            self.latest_db.insert_account_info(address, account_info);
        }
        Ok(self.latest_db.increase_balance(address, amount)?)
    }

    fn update(&mut self, address: B160, account: Account) {
        self.latest_db.update(address, account);
    }
}
