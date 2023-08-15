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

use std::{cell::RefCell, collections::BTreeSet};

use ethers_core::types::{EIP1186ProofResponse, H160, H256};
use hashbrown::HashMap;
use revm::{
    db::{CacheDB, DatabaseRef},
    primitives::{AccountInfo, Bytecode, B160, B256, U256},
    InMemoryDB,
};
use zeth_primitives::block::Header;

use crate::{
    auth_db::clone_storage_keys,
    host::provider::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery},
};

pub type CachedProviderDb = CacheDB<CacheDB<ProviderDb>>;

pub struct ProviderDb {
    pub provider: RefCell<Box<dyn Provider>>,
    pub block_no: u64,
    pub initial_db: RefCell<InMemoryDB>,
}

impl ProviderDb {
    pub fn new(provider: Box<dyn Provider>, block_no: u64) -> Self {
        ProviderDb {
            provider: RefCell::new(provider),
            block_no,
            initial_db: Default::default(),
        }
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
                self.provider.borrow_mut().get_proof(&ProofQuery {
                    block_no,
                    address,
                    indices,
                })?
            };
            out.insert(address, proof);
        }

        Ok(out)
    }
}

impl DatabaseRef for ProviderDb {
    type Error = anyhow::Error;

    fn basic(&self, address: B160) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(db_account) = self.initial_db.borrow().accounts.get(&address) {
            return Ok(db_account.info());
        }

        let account_info = {
            let address = H160::from(address.0);
            let query = AccountQuery {
                block_no: self.block_no,
                address,
            };
            let nonce = self.provider.borrow_mut().get_transaction_count(&query)?;
            let balance = self.provider.borrow_mut().get_balance(&query)?;
            let code = self.provider.borrow_mut().get_code(&query)?;

            AccountInfo::new(balance.into(), nonce.as_u64(), Bytecode::new_raw(code.0))
        };

        self.initial_db
            .borrow_mut()
            .insert_account_info(address, account_info.clone());

        Ok(Some(account_info))
    }

    fn code_by_hash(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    fn storage(&self, address: B160, index: U256) -> Result<U256, Self::Error> {
        // ensure that the corresponding account is loaded
        self.basic(address)?;

        if let Some(value) = self
            .initial_db
            .borrow()
            .accounts
            .get(&address)
            .unwrap()
            .storage
            .get(&index)
        {
            return Ok(*value);
        }

        let storage = {
            let address = H160::from(address.0);
            let bytes = index.to_be_bytes();
            let index = H256::from(bytes);

            let storage = self.provider.borrow_mut().get_storage(&StorageQuery {
                block_no: self.block_no,
                address,
                index,
            })?;
            U256::from_be_bytes(storage.0)
        };

        self.initial_db
            .borrow_mut()
            .insert_account_storage(address, index, storage)?;

        Ok(storage)
    }

    fn block_hash(&self, number: U256) -> Result<B256, Self::Error> {
        if let Some(hash) = self.initial_db.borrow().block_hashes.get(&number) {
            return Ok(*hash);
        }

        let block_no = u64::try_from(number).unwrap();
        let block_hash = self
            .provider
            .borrow_mut()
            .get_partial_block(&BlockQuery { block_no })?
            .hash
            .unwrap()
            .0
            .into();

        self.initial_db
            .borrow_mut()
            .block_hashes
            .insert(number, block_hash);

        Ok(block_hash)
    }
}

pub fn get_initial_proofs(
    latest_db: &mut CacheDB<ProviderDb>,
) -> Result<HashMap<B160, EIP1186ProofResponse>, anyhow::Error> {
    let provider_db = &mut latest_db.db;
    let storage_keys = clone_storage_keys(&provider_db.initial_db.borrow().accounts);
    provider_db.get_proofs(provider_db.block_no, storage_keys)
}

pub fn get_latest_proofs(
    latest_db: &mut CacheDB<ProviderDb>,
) -> Result<HashMap<B160, EIP1186ProofResponse>, anyhow::Error> {
    let mut storage_keys = clone_storage_keys(&latest_db.db.initial_db.borrow().accounts);

    for (address, mut indices) in clone_storage_keys(&latest_db.accounts) {
        match storage_keys.get_mut(&address) {
            Some(initial_indices) => initial_indices.append(&mut indices),
            None => {
                storage_keys.insert(address, indices);
            }
        }
    }

    let provider_db = &mut latest_db.db;
    provider_db.get_proofs(provider_db.block_no + 1, storage_keys)
}

pub fn get_ancestor_headers(
    latest_db: &mut CacheDB<ProviderDb>,
) -> Result<Vec<Header>, anyhow::Error> {
    let provider_db = &mut latest_db.db;
    let initial_db = provider_db.initial_db.borrow();
    let earliest_block = initial_db
        .block_hashes
        .keys()
        .min()
        .map(|uint| (*uint).try_into().unwrap())
        .unwrap_or(provider_db.block_no);
    let headers = (earliest_block..provider_db.block_no)
        .rev()
        .map(|block_no| {
            provider_db
                .provider
                .borrow_mut()
                .get_partial_block(&BlockQuery { block_no })
                .expect("Failed to retrieve ancestor block")
                .try_into()
                .expect("Failed to convert ethers block to zeth block")
        })
        .collect();
    Ok(headers)
}
