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

use alloy_rpc_types::EIP1186AccountProofResponse;
use ethers_core::types::{H160, H256};
use hashbrown::HashMap;
use revm::{
    primitives::{Account, AccountInfo, Bytecode},
    Database, DatabaseCommit,
};
use zeth_lib::mem_db::{DbError, MemDb};
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_bytes, from_ethers_u256},
    Address, B256, U256,
};

use super::provider::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery};

pub struct ProviderDb {
    pub provider: Box<dyn Provider>,
    pub block_no: u64,
    pub initial_db: MemDb,
    pub latest_db: MemDb,
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
        storage_keys: HashMap<Address, Vec<U256>>,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        let mut out = HashMap::new();

        for (address, indices) in storage_keys {
            let proof = {
                let address: H160 = address.into_array().into();
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
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        self.get_proofs(self.block_no, self.initial_db.storage_keys())
    }

    pub fn get_latest_proofs(
        &mut self,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
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

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
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
            let query = AccountQuery {
                block_no: self.block_no,
                address: address.into_array().into(),
            };
            let nonce = self.provider.get_transaction_count(&query)?;
            let balance = self.provider.get_balance(&query)?;
            let code = self.provider.get_code(&query)?;
            let bytecode = Bytecode::new_raw(from_ethers_bytes(code));

            AccountInfo::new(
                from_ethers_u256(balance),
                nonce.as_u64(),
                bytecode.hash_slow(),
                bytecode,
            )
        };

        self.initial_db
            .insert_account_info(address, account_info.clone());
        Ok(Some(account_info))
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
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
            let bytes = index.to_be_bytes();
            let index = H256::from(bytes);

            let storage = self.provider.get_storage(&StorageQuery {
                block_no: self.block_no,
                address: address.into_array().into(),
                index,
            })?;
            U256::from_be_bytes(storage.to_fixed_bytes())
        };

        self.initial_db
            .insert_account_storage(&address, index, storage);
        Ok(storage)
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

impl DatabaseCommit for ProviderDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.latest_db.commit(changes)
    }
}
