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

use anyhow::{bail, Result};
use hashbrown::HashMap;
use revm::{
    db::{CacheDB, DatabaseRef, DbAccount},
    primitives::{AccountInfo, Bytecode, B160, B256},
};
use ruint::aliases::U256;
use zeth_primitives::{
    keccak::keccak,
    trie::{MptNode, TrieAccount, EMPTY_ROOT},
};

#[derive(Clone, Debug)]
pub struct AuthenticatedDb {
    /// State trie of the block.
    pub state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub storage_tries: HashMap<B160, MptNode>,
}

impl DatabaseRef for AuthenticatedDb {
    type Error = anyhow::Error;

    fn basic(&self, address: B160) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(trie_account) = self.state_trie.get_rlp::<TrieAccount>(&keccak(address))? {
            if trie_account.storage_root != EMPTY_ROOT {
                bail!("Missing storage root!")
            }

            Ok(Some(trie_account.into()))
        } else {
            Ok(None)
        }
    }

    fn code_by_hash(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        unimplemented!()
    }

    fn storage(&self, address: B160, index: U256) -> Result<U256, Self::Error> {
        if let Some(storage_trie) = self.storage_tries.get(&address) {
            Ok(storage_trie
                .get_rlp(&keccak(index.to_be_bytes::<32>()))?
                .unwrap_or_default())
        } else {
            Ok(Default::default())
        }
    }

    fn block_hash(&self, _number: U256) -> Result<B256, Self::Error> {
        unimplemented!()
    }
}

pub type CachedAuthDb = CacheDB<AuthenticatedDb>;

pub fn clone_storage_keys(accounts: &HashMap<B160, DbAccount>) -> HashMap<B160, Vec<U256>> {
    accounts
        .iter()
        .map(|(address, account)| (*address, account.storage.keys().cloned().collect()))
        .collect()
}
