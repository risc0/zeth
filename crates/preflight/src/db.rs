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

use crate::driver::PreflightDriver;
use crate::provider::db::ProviderDB;
use crate::provider::get_proofs;
use crate::provider::query::BlockQuery;
use alloy::network::Network;
use alloy::primitives::map::HashMap;
use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::EIP1186AccountProofResponse;
use reth_primitives::revm_primitives::{Account, AccountInfo, Bytecode};
use reth_revm::db::states::StateChangeset;
use reth_revm::db::CacheDB;
use reth_revm::{Database, DatabaseCommit, DatabaseRef};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::ops::DerefMut;
use zeth_core::db::apply_changeset;
use zeth_core::driver::CoreDriver;
use zeth_core::rescue::{Recoverable, Rescued};

#[derive(Clone)]
pub struct MutCacheDB<T: DatabaseRef> {
    pub db: RefCell<CacheDB<T>>,
}

impl<T: DatabaseRef> MutCacheDB<T> {
    pub fn new(db: CacheDB<T>) -> Self {
        Self {
            db: RefCell::new(db),
        }
    }
}

impl<T: DatabaseRef> Database for MutCacheDB<T> {
    type Error = <CacheDB<T> as Database>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.db.borrow_mut().basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.db.borrow_mut().code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.db.borrow_mut().storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.db.borrow_mut().block_hash(number)
    }
}

impl<T: DatabaseRef> DatabaseRef for MutCacheDB<T> {
    type Error = <CacheDB<T> as DatabaseRef>::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.db.borrow_mut().basic(address)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.db.borrow_mut().code_by_hash(code_hash)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.db.borrow_mut().storage(address, index)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.db.borrow_mut().block_hash(number)
    }
}

pub type PrePostDB<N, R, P> = CacheDB<MutCacheDB<ProviderDB<N, R, P>>>;

#[derive(Clone)]
pub struct PreflightDB<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> {
    pub inner: PrePostDB<N, R, P>,
    pub driver: PhantomData<R>,
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> Recoverable for PreflightDB<N, R, P>
where
    R: Clone,
    P: Clone,
{
    fn rescue(&mut self) -> Option<Self> {
        Some(self.clone())
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> From<ProviderDB<N, R, P>>
    for PreflightDB<N, R, P>
{
    fn from(value: ProviderDB<N, R, P>) -> Self {
        Self {
            inner: CacheDB::new(MutCacheDB::new(CacheDB::new(value))),
            driver: PhantomData,
        }
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> From<PrePostDB<N, R, P>>
    for PreflightDB<N, R, P>
{
    fn from(value: PrePostDB<N, R, P>) -> Self {
        Self {
            inner: value,
            driver: PhantomData,
        }
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> From<Rescued<PrePostDB<N, R, P>>>
    for PreflightDB<N, R, P>
{
    fn from(value: Rescued<PrePostDB<N, R, P>>) -> Self {
        value.lock().unwrap().take().unwrap().into()
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> PreflightDB<N, R, P> {
    pub fn save_provider(&mut self) -> anyhow::Result<()> {
        self.inner.db.db.borrow_mut().db.save_provider()
    }

    pub fn advance_provider_block(&mut self) -> anyhow::Result<()> {
        self.inner.db.db.borrow_mut().db.advance_provider_block()
    }

    pub fn apply_changeset(&mut self, state_changeset: StateChangeset) -> anyhow::Result<()> {
        apply_changeset(&mut self.inner, state_changeset)
    }

    pub fn get_initial_proofs(
        &mut self,
    ) -> anyhow::Result<HashMap<Address, EIP1186AccountProofResponse>> {
        let initial_db = &self.inner.db;
        let storage_keys = enumerate_storage_keys(&initial_db.db.borrow());

        let initial_db = self.inner.db.db.borrow_mut();
        let block_no = initial_db.db.block_no;
        let res = get_proofs(
            initial_db.db.provider.borrow_mut().deref_mut(),
            block_no,
            storage_keys,
        )?;
        Ok(res)
    }

    pub fn get_latest_proofs(
        &mut self,
    ) -> anyhow::Result<HashMap<Address, EIP1186AccountProofResponse>> {
        // get initial keys
        let initial_db = &self.inner.db;
        let mut initial_storage_keys = enumerate_storage_keys(&initial_db.db.borrow());
        // merge initial keys with latest db storage keys
        for (address, mut indices) in enumerate_storage_keys(&self.inner) {
            match initial_storage_keys.get_mut(&address) {
                Some(initial_indices) => initial_indices.append(&mut indices),
                None => {
                    initial_storage_keys.insert(address, indices);
                }
            }
        }
        // return proofs as of next block
        let initial_db = self.inner.db.db.borrow_mut();
        let block_no = initial_db.db.block_no + 1;
        let res = get_proofs(
            initial_db.db.provider.borrow_mut().deref_mut(),
            block_no,
            initial_storage_keys,
        )?;
        Ok(res)
    }

    pub fn get_ancestor_headers(&mut self) -> anyhow::Result<Vec<N::HeaderResponse>> {
        let initial_db = &self.inner.db.db.borrow_mut();
        let db_block_number = initial_db.db.block_no;
        let earliest_block = initial_db
            .block_hashes
            .keys()
            .min()
            .copied()
            .map(|v| v.to())
            .unwrap_or(db_block_number);
        let mut provider = initial_db.db.provider.borrow_mut();
        let headers = (earliest_block..db_block_number)
            .rev()
            .map(|block_no| {
                P::derive_header_response(
                    provider
                        .get_full_block(&BlockQuery { block_no })
                        .expect("Failed to retrieve ancestor block"),
                )
            })
            .collect();
        Ok(headers)
    }
}

pub fn enumerate_storage_keys<T>(db: &CacheDB<T>) -> HashMap<Address, Vec<U256>> {
    db.accounts
        .iter()
        .map(|(address, account)| (*address, account.storage.keys().cloned().collect()))
        .collect()
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> Database for PreflightDB<N, R, P> {
    type Error = <PrePostDB<N, R, P> as Database>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash(number)
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> DatabaseRef for PreflightDB<N, R, P> {
    type Error = <PrePostDB<N, R, P> as DatabaseRef>::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic_ref(address)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash_ref(code_hash)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage_ref(address, index)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash_ref(number)
    }
}

impl<N: Network, R: CoreDriver, P: PreflightDriver<R, N>> DatabaseCommit for PreflightDB<N, R, P> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.inner.commit(changes)
    }
}
