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

use anyhow::Result;
use hashbrown::{hash_map, HashMap};
use revm::primitives::{Account, AccountInfo, Address, Bytecode, B160, B256, U256};
use zeth_primitives::{
    block::Header,
    keccak::{keccak, KECCAK_EMPTY},
    revm::to_revm_b256,
    trie::StateAccount,
    Bytes,
};

use crate::{
    consts::ChainSpec,
    execution::TxExecStrategy,
    finalization::BlockBuildStrategy,
    guest_mem_forget,
    mem_db::{AccountState, DbAccount},
    preparation::HeaderPrepStrategy,
    validation::{verify_parent_chain, verify_state_trie, verify_storage_trie, Input},
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

#[derive(Clone, Debug)]
pub struct BlockBuilder<'a, D> {
    pub(crate) chain_spec: &'a ChainSpec,
    pub(crate) input: Input,
    pub(crate) db: Option<D>,
    pub(crate) header: Option<Header>,
}

impl<D> BlockBuilder<'_, D>
where
    D: BlockBuilderDatabase,
    <D as revm::Database>::Error: core::fmt::Debug,
{
    /// Creates a new block builder.
    pub fn new(chain_spec: &ChainSpec, input: Input) -> BlockBuilder<'_, D> {
        BlockBuilder {
            chain_spec,
            db: None,
            header: None,
            input,
        }
    }

    /// Sets the database.
    pub fn with_db(mut self, db: D) -> Self {
        self.db = Some(db);
        self
    }

    /// Initializes the database from the input tries.
    pub fn initialize_db(mut self) -> Result<Self> {
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

    /// Initializes the header. This must be called before executing transactions.
    pub fn prepare_header<T: HeaderPrepStrategy>(self) -> Result<Self> {
        T::prepare_header(self)
    }

    /// Executes the transactions.
    pub fn execute_transactions<T: TxExecStrategy>(self) -> Result<Self> {
        T::execute_transactions(self)
    }

    /// Builds the block and returns the header.
    pub fn build<T: BlockBuildStrategy<Db = D>>(self) -> Result<T::Output> {
        T::build(self)
    }

    /// Returns a reference to the database.
    pub fn db(&self) -> Option<&D> {
        self.db.as_ref()
    }

    /// Returns a mutable reference to the database.
    pub fn mut_db(&mut self) -> Option<&mut D> {
        self.db.as_mut()
    }
}
