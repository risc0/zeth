// Copyright 2024, 2025 RISC Zero, Inc.
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

use crate::keccak::keccak;
use crate::map::NoMapHasher;
use crate::mpt::MptNode;
use crate::rescue::Recoverable;
use crate::stateless::data::StorageEntry;
use alloy_consensus::Account;
use alloy_primitives::map::{AddressHashMap, B256HashMap, HashMap};
use alloy_primitives::{Address, B256, U256};
use reth_primitives::revm_primitives::db::Database;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use reth_revm::DatabaseRef;
use reth_storage_errors::provider::ProviderError;

#[derive(Default)]
pub struct TrieDB {
    pub accounts: MptNode,
    pub storage: AddressHashMap<StorageEntry>,
    pub contracts: B256HashMap<Bytecode>,
    pub block_hashes: HashMap<u64, B256, NoMapHasher>,
}

impl Recoverable for TrieDB {
    fn rescue(&mut self) -> Option<Self> {
        Some(core::mem::take(self))
    }
}

impl DatabaseRef for TrieDB {
    type Error = ProviderError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        Ok(self
            .accounts
            .get_rlp::<Account>(&keccak(address))
            .unwrap()
            .map(|acc| AccountInfo {
                balance: acc.balance,
                nonce: acc.nonce,
                code_hash: acc.code_hash,
                code: None,
            }))
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        Ok(self.contracts.get(&code_hash).unwrap().clone())
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let entry = self.storage.get(&address).unwrap();
        Ok(entry
            .storage_trie
            .get_rlp(&keccak(index.to_be_bytes::<32>()))
            .unwrap()
            .unwrap_or_default())
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        Ok(*self.block_hashes.get(&number).unwrap())
    }
}

impl Database for TrieDB {
    type Error = ProviderError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.basic_ref(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.code_by_hash_ref(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.storage_ref(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.block_hash_ref(number)
    }
}
