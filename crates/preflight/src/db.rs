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

use crate::provider::db::ProviderDb;
use crate::provider::{get_proofs, BlockQuery, UncleQuery};
use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::{EIP1186AccountProofResponse, Header};
use hashbrown::HashMap;
use reth_revm::db::CacheDB;

pub type PreflightDb = CacheDB<CacheDB<ProviderDb>>;

impl From<ProviderDb> for PreflightDb {
    fn from(value: ProviderDb) -> Self {
        CacheDB::new(CacheDB::new(value))
    }
}

pub fn enumerate_storage_keys<T>(db: &CacheDB<T>) -> HashMap<Address, Vec<U256>> {
    db.accounts
        .iter()
        .map(|(address, account)| (*address, account.storage.keys().cloned().collect()))
        .collect()
}

pub fn get_initial_proofs(
    db: &mut PreflightDb,
) -> anyhow::Result<HashMap<Address, EIP1186AccountProofResponse>> {
    let initial_db = &db.db;
    let storage_keys = enumerate_storage_keys(initial_db);

    get_proofs(
        db.db.db.provider.get_mut().as_mut(),
        db.db.db.block_no,
        storage_keys,
    )
}

pub fn get_latest_proofs(
    db: &mut PreflightDb,
) -> anyhow::Result<HashMap<Address, EIP1186AccountProofResponse>> {
    // get initial keys
    let initial_db = &db.db;
    let mut storage_keys = enumerate_storage_keys(initial_db);
    // merge initial keys with latest db storage keys
    for (address, mut indices) in enumerate_storage_keys(db) {
        match storage_keys.get_mut(&address) {
            Some(initial_indices) => initial_indices.append(&mut indices),
            None => {
                storage_keys.insert(address, indices);
            }
        }
    }
    // return proofs as of next block
    get_proofs(
        db.db.db.provider.get_mut().as_mut(),
        db.db.db.block_no + 1,
        storage_keys,
    )
}

pub fn get_ancestor_headers(db: &mut PreflightDb) -> anyhow::Result<Vec<Header>> {
    let initial_db = &db.db;
    let db_block_number = db.db.db.block_no;
    let earliest_block = initial_db
        .block_hashes
        .keys()
        .min()
        .copied()
        .map(|v| v.to())
        .unwrap_or(db_block_number);
    let headers = (earliest_block..db_block_number)
        .rev()
        .map(|block_no| {
            db.db
                .db
                .provider
                .get_mut()
                .get_full_block(&BlockQuery { block_no })
                .expect("Failed to retrieve ancestor block")
                .header
        })
        .collect();
    Ok(headers)
}

pub fn get_uncles(db: &mut PreflightDb, uncle_hashes: &Vec<B256>) -> anyhow::Result<Vec<Header>> {
    let provider = db.db.db.provider.get_mut().as_mut();
    let ommers = uncle_hashes
        .into_iter()
        .enumerate()
        .map(|(index, uncle_hash)| {
            provider
                .get_uncle_block(&UncleQuery {
                    uncle_hash: *uncle_hash,
                    index_number: index as u64,
                })
                .expect("Failed to retrieve uncle block")
                .header
        })
        .collect();
    Ok(ommers)
}
