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

use anyhow::{anyhow, bail, Result};
use hashbrown::HashMap;
use revm::{
    db::{CacheDB, DatabaseRef, DbAccount},
    primitives::{AccountInfo, Bytecode, B160, B256, KECCAK_EMPTY},
};
use ruint::aliases::U256;
use zeth_primitives::{
    block::Header,
    keccak::keccak,
    revm::to_revm_b256,
    trie::{MptNode, TrieAccount},
    Bytes,
};

#[derive(Clone, Debug)]
pub struct AuthenticatedDb {
    /// State trie of the block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub storage_tries: HashMap<B160, MptNode>,
    /// Maps bytecode digests to their preimages
    pub contracts: HashMap<B256, Bytecode>,
    /// Maps block numbers to their hashes
    pub block_hashes: HashMap<U256, B256>,
}

impl DatabaseRef for AuthenticatedDb {
    type Error = anyhow::Error;

    fn basic(&self, address: B160) -> Result<Option<AccountInfo>, Self::Error> {
        Ok(self
            .state_trie
            .get_rlp::<TrieAccount>(&keccak(address))?
            .map(|trie_account| trie_account.into()))
    }

    fn code_by_hash(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.contracts
            .get(&code_hash)
            .cloned()
            .ok_or(anyhow!("Missing bytecode!"))
    }

    fn storage(&self, address: B160, index: U256) -> Result<U256, Self::Error> {
        Ok(self
            .storage_tries
            .get(&address)
            .ok_or(anyhow!("Missing account storage trie!"))?
            .get_rlp(&keccak(index.to_be_bytes::<32>()))?
            .unwrap_or_default())
    }

    fn block_hash(&self, number: U256) -> Result<B256, Self::Error> {
        self.block_hashes
            .get(&number)
            .ok_or(anyhow!("Missing block hash!"))
            .cloned()
    }
}

impl AuthenticatedDb {
    pub fn new(
        state_trie: MptNode,
        storage_tries: HashMap<B160, MptNode>,
        contracts: Vec<Bytes>,
        blocks: Vec<&Header>,
    ) -> Result<Self> {
        // authenticate account storage tries
        for (address, storage_trie) in &storage_tries {
            let account = state_trie
                .get_rlp::<TrieAccount>(&keccak(address))?
                .unwrap_or_default();
            if storage_trie.hash() != account.storage_root {
                bail!("Initial account storage root mismatch!")
            }
        }
        // authenticate historical block hashes
        let mut block_hashes: HashMap<U256, B256> = Default::default();
        blocks.into_iter().fold(Ok(None), |previous, current| {
            if let Ok(Some(parent_hash)) = previous {
                if parent_hash != current.parent_hash {
                    bail!("Invalid historical block sequence!")
                }
            }
            let current_block_hash = current.hash();
            block_hashes.insert(current.number.try_into()?, to_revm_b256(current_block_hash));
            Ok(Some(current_block_hash))
        })?;
        Ok(AuthenticatedDb {
            state_trie,
            storage_tries,
            contracts: contracts
                .into_iter()
                .map(|bytes| unsafe {
                    let hash = keccak(&bytes).into();
                    (hash, Bytecode::new_raw_with_hash(bytes.0, hash))
                })
                .collect(),
            block_hashes,
        })
    }
}

impl Into<CacheDB<AuthenticatedDb>> for AuthenticatedDb {
    fn into(mut self) -> CacheDB<AuthenticatedDb> {
        let mut contracts = core::mem::take(&mut self.contracts);
        contracts.insert(KECCAK_EMPTY, Bytecode::new());
        contracts.insert(B256::zero(), Bytecode::new());
        CacheDB {
            accounts: Default::default(),
            contracts,
            logs: vec![],
            block_hashes: core::mem::take(&mut self.block_hashes),
            db: self,
        }
    }
}

pub type CachedAuthDb = CacheDB<AuthenticatedDb>;

pub fn clone_storage_keys(accounts: &HashMap<B160, DbAccount>) -> HashMap<B160, Vec<U256>> {
    accounts
        .iter()
        .map(|(address, account)| (*address, account.storage.keys().cloned().collect()))
        .collect()
}