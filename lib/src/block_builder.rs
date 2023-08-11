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

use core::mem;

use anyhow::{bail, Result};
use hashbrown::{hash_map, HashMap};
use revm::primitives::{Account, AccountInfo, Address, Bytecode, B160, B256, U256};
use zeth_primitives::{
    block::Header,
    keccak::{keccak, KECCAK_EMPTY},
    revm::{from_revm_b256, to_revm_b256},
    trie::{MptNode, StateAccount},
    Bytes,
};

use crate::{
    consts::ChainSpec,
    execution::TxExecStrategy,
    guest_mem_forget,
    mem_db::{AccountState, DbAccount, MemDb},
    validation::{
        compute_base_fee, compute_block_number, verify_extra_data, verify_gas_limit,
        verify_parent_chain, verify_state_trie, verify_storage_trie, verify_timestamp, Input,
    },
};

pub trait BlockBuilderDatabase: revm::Database + Sized {
    /// Creates a new DB from the accounts and the block hashes.
    fn load(accounts: HashMap<B160, DbAccount>, block_hashes: HashMap<u64, B256>) -> Self;
    /// Returns all non-deleted accounts with their storage entries.
    fn accounts(&self) -> hash_map::Iter<B160, DbAccount>;
    /// Increases the balance of `address` by `amount`.
    fn increase_balance(&mut self, address: Address, amount: U256) -> Result<(), Self::Error>;
    /// Updates the account of `address`.
    fn update(&mut self, address: Address, account: Account);
}

#[derive(Clone)]
pub struct BlockBuilder<D> {
    pub chain_spec: Option<ChainSpec>,
    pub db: Option<D>,
    pub header: Option<Header>,
    pub input: Input,
}

impl From<Input> for BlockBuilder<MemDb> {
    fn from(input: Input) -> Self {
        BlockBuilder {
            chain_spec: None,
            db: None,
            header: None,
            input,
        }
    }
}

impl<D> BlockBuilder<D>
where
    D: BlockBuilderDatabase,
    <D as revm::Database>::Error: std::fmt::Debug,
{
    pub fn new(chain_spec: Option<ChainSpec>, db: Option<D>, input: Input) -> Self {
        BlockBuilder {
            chain_spec,
            db,
            header: None,
            input,
        }
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }

    pub fn with_chain_spec(mut self, chain_spec: ChainSpec) -> Self {
        self.chain_spec = Some(chain_spec);
        self
    }

    pub fn initialize_evm_storage(mut self) -> Result<Self> {
        verify_state_trie(
            &self.input.parent_state_trie,
            &self.input.parent_header.state_root,
        )?;

        // hash all the contract code
        let contracts: HashMap<B256, Bytes> = mem::take(&mut self.input.contracts)
            .into_iter()
            .map(|bytes| (keccak(&bytes).into(), bytes))
            .collect();

        // Load account data into db
        let mut accounts = HashMap::with_capacity(self.input.parent_storage.len());
        for (address, (storage_trie, slots)) in &mut self.input.parent_storage {
            // consume the slots, as they are no longer needed afterwards
            let slots = mem::take(slots);

            // load the account from the state trie or empty if it does not exist
            let state_account = self
                .input
                .parent_state_trie
                .get_rlp::<StateAccount>(&keccak(address))?
                .unwrap_or_default();
            verify_storage_trie(address, storage_trie, &state_account.storage_root)?;

            // load the corresponding code
            let code_hash = to_revm_b256(state_account.code_hash);
            let bytecode = if code_hash.0 == KECCAK_EMPTY.0 {
                Bytecode::new()
            } else {
                let bytes = contracts.get(&code_hash).unwrap().clone();
                unsafe { Bytecode::new_raw_with_hash(bytes.0, code_hash) }
            };

            // load storage reads
            let mut storage = HashMap::with_capacity(slots.len());
            for slot in slots {
                let value: zeth_primitives::U256 = storage_trie
                    .get_rlp(&keccak(slot.to_be_bytes::<32>()))?
                    .unwrap_or_default();
                storage.insert(slot, value);
            }

            let mem_account = DbAccount {
                info: AccountInfo {
                    balance: state_account.balance,
                    nonce: state_account.nonce,
                    code_hash: to_revm_b256(state_account.code_hash),
                    code: Some(bytecode),
                },
                state: AccountState::None,
                storage,
            };

            accounts.insert(*address, mem_account);
        }
        guest_mem_forget(contracts);

        // prepare block hash history
        let block_hashes =
            verify_parent_chain(&self.input.parent_header, &self.input.ancestor_headers)?;

        // Store database
        self.db = Some(D::load(accounts, block_hashes));

        Ok(self)
    }

    pub fn initialize_header(mut self) -> Result<Self> {
        // Verify current block
        verify_gas_limit(self.input.gas_limit, self.input.parent_header.gas_limit)?;
        verify_timestamp(self.input.timestamp, self.input.parent_header.timestamp)?;
        verify_extra_data(&self.input.extra_data)?;
        // Initialize result header
        let Some(ref chain_spec) = self.chain_spec else {
            bail!("Missing ChainSpec");
        };
        self.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: self.input.parent_header.hash(),
            number: compute_block_number(&self.input.parent_header)?,
            base_fee_per_gas: compute_base_fee(
                &self.input.parent_header,
                chain_spec.gas_constants(),
            )?,
            // Initialize metadata from input
            beneficiary: self.input.beneficiary,
            gas_limit: self.input.gas_limit,
            timestamp: self.input.timestamp,
            mix_hash: self.input.mix_hash,
            extra_data: self.input.extra_data.clone(),
            // do not fill the remaining fields
            ..Default::default()
        });
        Ok(self)
    }

    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    pub fn build(
        mut self,
        mut debug_storage_tries: Option<&mut HashMap<Address, MptNode>>,
    ) -> Result<Header> {
        let db = self.db.as_ref().unwrap();

        // apply state updates
        let state_trie = &mut self.input.parent_state_trie;
        for (address, account) in db.accounts() {
            // if the account has not been touched, it can be ignored
            if account.state == AccountState::None {
                if let Some(map) = &mut debug_storage_tries {
                    let storage_root = self.input.parent_storage.get(address).unwrap().0.clone();
                    map.insert(*address, storage_root);
                }
                continue;
            }

            // compute the index of the current account in the state trie
            let state_trie_index = keccak(address);

            // remove deleted accounts from the state trie
            if account.state == AccountState::Deleted {
                state_trie.delete(&state_trie_index)?;
                continue;
            }

            // otherwise, compute the updated storage root for that account
            let state_storage = &account.storage;
            let storage_root = {
                // getting a mutable reference is more efficient than calling remove
                // every account must have an entry, even newly created accounts
                let (storage_trie, _) = self.input.parent_storage.get_mut(address).unwrap();
                // for cleared accounts always start from the empty trie
                if account.state == AccountState::StorageCleared {
                    storage_trie.clear();
                }

                // apply all new storage entries for the current account (address)
                for (key, value) in state_storage {
                    let storage_trie_index = keccak(key.to_be_bytes::<32>());
                    if value == &U256::ZERO {
                        storage_trie.delete(&storage_trie_index)?;
                    } else {
                        storage_trie.insert_rlp(&storage_trie_index, *value)?;
                    }
                }

                // insert the storage trie for host debugging
                if let Some(map) = &mut debug_storage_tries {
                    map.insert(*address, storage_trie.clone());
                }

                storage_trie.hash()
            };

            let state_account = StateAccount {
                nonce: account.info.nonce,
                balance: account.info.balance,
                storage_root,
                code_hash: from_revm_b256(account.info.code_hash),
            };
            state_trie.insert_rlp(&state_trie_index, state_account)?;
        }

        // update result header with the new state root
        let mut header = self.header.take().expect("Header was not initialized");
        header.state_root = state_trie.hash();

        // Leak memory, save cycles
        guest_mem_forget(self);

        Ok(header)
    }
}
