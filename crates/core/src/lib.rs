// Copyright 2025 RISC Zero, Inc.
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

use alloy_consensus::Header;
use alloy_primitives::{Address, B256, Bytes, KECCAK256_EMPTY, U256, keccak256, map::B256Map};
use alloy_trie::{EMPTY_ROOT_HASH, TrieAccount};
use reth_chainspec::{EthChainSpec, Hardforks};
use reth_errors::ProviderError;
use reth_ethereum_primitives::Block;
use reth_evm::{EthEvmFactory, eth::spec::EthExecutorSpec};
use reth_stateless::validation::StatelessValidationError;
use reth_trie_common::HashedPostState;
use revm_bytecode::Bytecode;
use risc0_ethereum_trie::CachedTrie;
use std::{cell::RefCell, collections::hash_map::Entry, fmt::Debug, marker::PhantomData};

pub use reth_stateless::{ExecutionWitness, StatelessInput, StatelessTrie};

pub type EthEvmConfig<C> = reth_evm_ethereum::EthEvmConfig<C, EthEvmFactory>;

#[inline]
pub fn validate_block<C>(
    block: Block,
    witness: ExecutionWitness,
    config: EthEvmConfig<C>,
) -> Result<B256, StatelessValidationError>
where
    C: EthExecutorSpec + EthChainSpec<Header = Header> + Hardforks + 'static,
{
    reth_stateless::stateless_validation_with_trie::<SparseState, _, _>(
        block,
        witness,
        config.chain_spec().clone(),
        config,
    )
}

#[derive(Debug, Clone, Default)]
#[repr(transparent)]
struct RlpTrie<T> {
    inner: CachedTrie,
    phantom: PhantomData<T>,
}

impl<T: alloy_rlp::Decodable + alloy_rlp::Encodable> RlpTrie<T> {
    fn new(inner: CachedTrie) -> Self {
        Self { inner, phantom: PhantomData }
    }

    pub fn from_prehashed(
        root: B256,
        rlp_by_digest: &B256Map<impl AsRef<[u8]>>,
    ) -> alloy_rlp::Result<Self> {
        Ok(Self::new(CachedTrie::from_prehashed_nodes(root, rlp_by_digest)?))
    }

    pub fn get(&self, key: impl AsRef<[u8]>) -> alloy_rlp::Result<Option<T>> {
        self.inner.get(key).map(alloy_rlp::decode_exact).transpose()
    }

    pub fn insert(&mut self, key: impl AsRef<[u8]>, value: T) {
        self.inner.insert(key, alloy_rlp::encode(value));
    }

    pub fn remove(&mut self, key: impl AsRef<[u8]>) -> bool {
        self.inner.remove(key)
    }

    pub fn hash(&mut self) -> B256 {
        self.inner.hash()
    }
}

#[derive(Debug, Clone)]
struct SparseState {
    state: RlpTrie<TrieAccount>,
    storage: RefCell<B256Map<RlpTrie<U256>>>,

    rlp_by_digest: B256Map<Bytes>,
}

impl SparseState {
    fn remove_account(&mut self, hashed_address: &B256) {
        self.state.remove(hashed_address);
        self.storage.get_mut().remove(hashed_address);
    }

    fn clear_storage(&mut self, hashed_address: B256) -> &mut RlpTrie<U256> {
        self.storage.get_mut().entry(hashed_address).insert_entry(RlpTrie::default()).into_mut()
    }

    fn storage_trie_mut(&mut self, hashed_address: B256) -> alloy_rlp::Result<&mut RlpTrie<U256>> {
        let trie = match self.storage.get_mut().entry(hashed_address) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let storage_root =
                    self.state.get(hashed_address)?.map_or(EMPTY_ROOT_HASH, |a| a.storage_root);
                entry.insert(RlpTrie::from_prehashed(storage_root, &self.rlp_by_digest)?)
            }
        };

        Ok(trie)
    }
}

impl StatelessTrie for SparseState {
    fn new(
        witness: &ExecutionWitness,
        pre_state_root: B256,
    ) -> Result<(Self, B256Map<Bytecode>), StatelessValidationError> {
        let rlp_by_digest: B256Map<_> =
            witness.state.iter().map(|rlp| (keccak256(rlp), rlp.clone())).collect();

        let state = RlpTrie::from_prehashed(pre_state_root, &rlp_by_digest)
            .map_err(|_| StatelessValidationError::WitnessRevealFailed { pre_state_root })?;

        let bytecode = witness
            .codes
            .iter()
            .map(|code| (keccak256(code), Bytecode::new_raw(code.clone())))
            .collect();

        Ok((Self { state, storage: RefCell::new(B256Map::default()), rlp_by_digest }, bytecode))
    }

    fn account(&self, address: Address) -> Result<Option<TrieAccount>, ProviderError> {
        let hashed_address = keccak256(address);
        match self.state.get(hashed_address)? {
            None => Ok(None),
            Some(account) => {
                match self.storage.borrow_mut().entry(hashed_address) {
                    Entry::Vacant(entry) => {
                        entry.insert(RlpTrie::from_prehashed(
                            account.storage_root,
                            &self.rlp_by_digest,
                        )?);
                    }
                    Entry::Occupied(_) => {}
                }

                Ok(Some(account))
            }
        }
    }

    fn storage(&self, address: Address, slot: U256) -> Result<U256, ProviderError> {
        let storage = self.storage.borrow();
        let trie = storage.get(&keccak256(address)).unwrap();
        Ok(trie.get(keccak256(B256::from(slot)))?.unwrap_or(U256::ZERO))
    }

    fn calculate_state_root(
        &mut self,
        state: HashedPostState,
    ) -> Result<B256, StatelessValidationError> {
        let mut removed_accounts = Vec::new();
        for (hashed_address, account) in state.accounts {
            let Some(account) = account else {
                removed_accounts.push(hashed_address);
                continue;
            };

            let storage_root = match state.storages.get(&hashed_address) {
                None => self.storage_trie_mut(hashed_address).unwrap().hash(),
                Some(storage) => {
                    let storage_trie = if storage.wiped {
                        self.clear_storage(hashed_address)
                    } else {
                        self.storage_trie_mut(hashed_address).unwrap()
                    };

                    // always remove from trie first, otherwise nodes might not be fully resolved
                    for (hashed_key, value) in &storage.storage {
                        if !value.is_zero() {
                            storage_trie.insert(hashed_key, *value);
                        }
                    }
                    for (hashed_key, value) in &storage.storage {
                        if value.is_zero() {
                            storage_trie.remove(hashed_key);
                        }
                    }

                    storage_trie.hash()
                }
            };

            let account = TrieAccount {
                nonce: account.nonce,
                balance: account.balance,
                storage_root,
                code_hash: account.bytecode_hash.unwrap_or(KECCAK256_EMPTY),
            };
            self.state.insert(hashed_address, account);
        }
        removed_accounts.iter().for_each(|hashed_address| self.remove_account(hashed_address));

        Ok(self.state.hash())
    }
}
