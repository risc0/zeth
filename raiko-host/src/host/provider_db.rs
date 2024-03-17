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
use alloy_rpc_types::{BlockId, EIP1186AccountProofResponse};
use hashbrown::HashMap;
use revm::{
    primitives::{Account, AccountInfo, Bytecode},
    Database, DatabaseCommit,
};
use tokio::runtime::Handle;
use zeth_lib::{taiko_utils::to_header, mem_db::{DbError, MemDb}};
use zeth_primitives::{
    Address, B256, U256,
};
use alloy_consensus::Header as AlloyConsensusHeader;
use crate::host::host::get_block_alloy;
use alloy_providers::tmp::{HttpProvider, TempProvider};

pub struct ProviderDb {
    pub provider: HttpProvider,
    pub block_number: u64,
    pub initial_db: MemDb,
    pub current_db: MemDb,
    async_executor: Handle,
}

impl ProviderDb {
    pub fn new(provider: HttpProvider, block_number: u64) -> Self {
        ProviderDb {
            provider,
            block_number,
            initial_db: MemDb::default(),
            current_db: MemDb::default(),
            async_executor: tokio::runtime::Handle::current(),
        }
    }

    pub fn get_initial_db(&self) -> &MemDb {
        &self.initial_db
    }

    pub fn get_latest_db(&self) -> &MemDb {
        &self.current_db
    }

    fn get_proofs(
        &mut self,
        block_number: u64,
        storage_keys: HashMap<Address, Vec<U256>>,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        let mut storage_proofs = HashMap::new();
        for (address, keys) in storage_keys {
            let indices = keys.into_iter().map(|x| x.to_be_bytes().into()).collect();
            let proof = self.async_executor.block_on(async {
                self.provider
                    .get_proof(address, indices, Some(BlockId::from(block_number)))
                    .await
            })?;
            storage_proofs.insert(address, proof);
        }
        Ok(storage_proofs)
    }

    pub fn get_initial_proofs(
        &mut self,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        self.get_proofs(self.block_number, self.initial_db.storage_keys())
    }

    pub fn get_latest_proofs(
        &mut self,
    ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
        let mut storage_keys = self.initial_db.storage_keys();
        for (address, mut indices) in self.current_db.storage_keys() {
            match storage_keys.get_mut(&address) {
                Some(initial_indices) => initial_indices.append(&mut indices),
                None => {
                    storage_keys.insert(address, indices);
                }
            }
        }
        self.get_proofs(self.block_number + 1, storage_keys)
    }

    pub fn get_ancestor_headers(&mut self, rpc_url: String) -> Result<Vec<AlloyConsensusHeader>, anyhow::Error> {
        let earliest_block = self
            .initial_db
            .block_hashes
            .keys()
            .min()
            .unwrap_or(&self.block_number);
        let headers = (*earliest_block..self.block_number)
            .rev()
            .map(|block_no| {
                to_header(&get_block_alloy(rpc_url.clone(), block_no, false).unwrap().header)
            })
            .collect();
        Ok(headers)
    }
}

impl Database for ProviderDb {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Check if the account is in the current database.
        if let Ok(db_result) = self.current_db.basic(address) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.basic(address) {
            return Ok(db_result);
        }

        // Get the nonce, balance, and code to reconstruct the account.
        let nonce = self.async_executor.block_on(async {
            self.provider
                .get_transaction_count(address, Some(BlockId::from(self.block_number)))
                .await
        })?;
        let balance = self.async_executor.block_on(async {
            self.provider
                .get_balance(address, Some(BlockId::from(self.block_number)))
                .await
        })?;
        let code = self.async_executor.block_on(async {
            self.provider
                .get_code_at(address, Some(BlockId::from(self.block_number)))
                .await
        })?;

        // Insert the account into the initial database.
        let account_info = AccountInfo::new(
            balance,
            nonce.try_into().unwrap(),
            Bytecode::new_raw(code.clone()).hash_slow(),
            Bytecode::new_raw(code),
        );
        self.initial_db
            .insert_account_info(address, account_info.clone());
        Ok(Some(account_info))
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        // Check if the storage slot is in the current database.
        if let Ok(db_result) = self.current_db.storage(address, index) {
            return Ok(db_result);
        }
        if let Ok(db_result) = self.initial_db.storage(address, index) {
            return Ok(db_result);
        }

        // Get the storage slot from the provider.
        self.initial_db.basic(address)?;
        let storage = self.async_executor.block_on(async {
            self.provider
                .get_storage_at(
                    address.into_array().into(),
                    index,
                    Some(BlockId::from(self.block_number)),
                )
                .await
        })?;
        self.initial_db
            .insert_account_storage(&address, index, storage);
        Ok(storage)
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        // Check if the block hash is in the current database.
        if let Ok(block_hash) = self.initial_db.block_hash(number) {
            return Ok(block_hash);
        }

        // Get the block hash from the provider.
        let block_number = u64::try_from(number).unwrap();
        let block_hash = self.async_executor.block_on(async {
            self.provider
                .get_block_by_number(block_number.into(), false)
                .await
                .unwrap()
                .unwrap()
                .header
                .hash
                .unwrap()
                .0
                .into()
        });
        self.initial_db
            .insert_block_hash(block_number, block_hash);
        Ok(block_hash)
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        unreachable!()
    }
}

impl DatabaseCommit for ProviderDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.current_db.commit(changes)
    }
}
