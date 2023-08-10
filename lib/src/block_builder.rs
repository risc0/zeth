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

use anyhow::{anyhow, bail, Context, Result};
use hashbrown::{hash_map, HashMap};
#[cfg(not(target_os = "zkvm"))]
use log::{debug, info};
use revm::{
    primitives::{
        Account, AccountInfo, Address, BlockEnv, Bytecode, CfgEnv, ResultAndState, SpecId,
        TransactTo, TxEnv, B160, B256, U256,
    },
    EVM,
};
use zeth_primitives::{
    block::Header,
    keccak::{keccak, KECCAK_EMPTY},
    receipt::Receipt,
    revm::{from_revm_b256, to_revm_b160, to_revm_b256},
    transaction::{Transaction, TransactionKind, TxEssence},
    trie::{MptNode, StateAccount},
    Bloom, Bytes, RlpBytes,
};

use crate::{
    consts::{self, GWEI_TO_WEI},
    guest_mem_forget,
    mem_db::{AccountState, DbAccount, MemDb},
    validation::{
        compute_base_fee, compute_block_number, compute_spec_id, verify_extra_data,
        verify_gas_limit, verify_parent_chain, verify_state_trie, verify_storage_trie,
        verify_timestamp, Input,
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
    db: Option<D>,
    header: Option<Header>,
    input: Input,
}

impl From<Input> for BlockBuilder<MemDb> {
    fn from(input: Input) -> Self {
        BlockBuilder {
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
    pub fn new(db: Option<D>, input: Input) -> Self {
        BlockBuilder {
            db,
            header: None,
            input,
        }
    }

    pub fn to_db(self) -> D {
        self.db.unwrap()
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
        self.header = Some(Header {
            // Initialize fields that we can compute from the parent
            parent_hash: self.input.parent_header.hash(),
            number: compute_block_number(&self.input.parent_header)?,
            base_fee_per_gas: compute_base_fee(
                &self.input.parent_header,
                self.input.chain_spec.gas_constants(),
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

    pub fn execute_transactions(mut self) -> Result<Self> {
        let header = self.header.as_mut().expect("Header is not initialized");
        let spec_id = compute_spec_id(&self.input.chain_spec, header.number)?;

        #[cfg(not(target_os = "zkvm"))]
        {
            use chrono::{TimeZone, Utc};
            let dt = Utc
                .timestamp_opt(self.input.timestamp.try_into().unwrap(), 0)
                .unwrap();

            info!("Block no. {}", header.number);
            info!("  EVM spec ID: {:?}", spec_id);
            info!("  Timestamp: {}", dt);
            info!("  Transactions: {}", self.input.transactions.len());
            info!("  Withdrawals: {}", self.input.withdrawals.len());
            info!("  Fee Recipient: {:?}", self.input.beneficiary);
            info!("  Gas limit: {}", self.input.gas_limit);
            info!("  Base fee per gas: {}", header.base_fee_per_gas);
            info!("  Extra data: {:?}", self.input.extra_data);
        }

        // initialize the EVM
        let mut evm = EVM::new();

        evm.env.cfg = CfgEnv {
            chain_id: U256::from(self.input.chain_spec.chain_id()),
            spec_id,
            ..Default::default()
        };
        evm.env.block = BlockEnv {
            number: header.number.try_into().unwrap(),
            coinbase: to_revm_b160(self.input.beneficiary),
            timestamp: self.input.timestamp,
            difficulty: U256::ZERO,
            prevrandao: Some(to_revm_b256(self.input.mix_hash)),
            basefee: header.base_fee_per_gas,
            gas_limit: self.input.gas_limit,
        };

        evm.database(self.db.take().unwrap());

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = consts::ZERO;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in self.input.transactions.iter().enumerate() {
            // verify the transaction signature
            let tx_from = tx
                .recover_from()
                .with_context(|| format!("Error recovering address for transaction {}", tx_no))?;

            #[cfg(not(target_os = "zkvm"))]
            {
                let tx_hash = tx.hash();
                debug!("Tx no. {} (hash: {})", tx_no, tx_hash);
                debug!("  Type: {}", tx.tx_type());
                debug!("  Fr: {:?}", tx_from);
                debug!("  To: {:?}", tx.to().unwrap_or_default());
            }

            // verify transaction gas
            let block_available_gas = self.input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx.gas_limit() {
                bail!("Error at transaction {}: gas exceeds block limit", tx_no);
            }

            // process the transaction
            let tx_from = to_revm_b160(tx_from);
            fill_tx_env(&mut evm.env.tx, tx, tx_from);
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))?;

            let gas_used = result.gas_used().try_into().unwrap();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            #[cfg(not(target_os = "zkvm"))]
            debug!("  Ok: {:?}", result);

            // create the receipt from the EVM result
            let receipt = Receipt::new(
                tx.tx_type(),
                result.is_success(),
                cumulative_gas_used,
                result.logs().into_iter().map(|log| log.into()).collect(),
            );

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(receipt.payload.logs_bloom);

            // Add receipt and tx to tries
            let trie_key = tx_no.to_rlp();
            tx_trie
                .insert_rlp(&trie_key, tx)
                .context("failed to insert transaction")?;
            receipt_trie
                .insert_rlp(&trie_key, receipt)
                .context("failed to insert receipt")?;

            // update account states
            let db = evm.db().unwrap();
            for (address, account) in state {
                #[cfg(not(target_os = "zkvm"))]
                if account.is_touched {
                    // log account
                    debug!(
                        "  State {:?} (storage_cleared={}, is_destroyed={}, is_not_existing={})",
                        address,
                        account.storage_cleared,
                        account.is_destroyed,
                        account.is_not_existing
                    );
                    // log balance changes
                    debug!(
                        "     After balance: {} (Nonce: {})",
                        account.info.balance, account.info.nonce
                    );

                    // log state changes
                    for (addr, slot) in &account.storage {
                        if slot.is_changed() {
                            debug!("    Storage address: {:?}", addr);
                            debug!("      Before: {:?}", slot.original_value);
                            debug!("       After: {:?}", slot.present_value);
                        }
                    }
                }

                db.update(address, account);
            }
        }

        self.db = Some(evm.take_db());

        // process withdrawals unconditionally after any transactions
        let mut withdrawals_trie = MptNode::default();
        for (i, withdrawal) in self.input.withdrawals.iter().enumerate() {
            // the withdrawal amount is given in Gwei
            let amount_wei = GWEI_TO_WEI
                .checked_mul(withdrawal.amount.try_into().unwrap())
                .unwrap();

            #[cfg(not(target_os = "zkvm"))]
            {
                debug!("Withdrawal no. {}", withdrawal.index);
                debug!("  Recipient: {:?}", withdrawal.address);
                debug!("  Value: {}", amount_wei);
            }

            self.db
                .as_mut()
                .unwrap()
                .increase_balance(to_revm_b160(withdrawal.address), amount_wei)
                .unwrap();

            // add to trie
            withdrawals_trie
                .insert_rlp(&i.to_rlp(), withdrawal)
                .context("failed to insert withdrawal")?;
        }

        // Update result header with computed values
        header.transactions_root = tx_trie.hash();
        header.receipts_root = receipt_trie.hash();
        header.logs_bloom = logs_bloom;
        header.gas_used = cumulative_gas_used;
        header.withdrawals_root = if spec_id < SpecId::SHANGHAI {
            None
        } else {
            Some(withdrawals_trie.hash())
        };

        // Leak memory, save cycles
        guest_mem_forget([tx_trie, receipt_trie, withdrawals_trie]);

        Ok(self)
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

fn fill_tx_env(tx_env: &mut TxEnv, tx: &Transaction, caller: Address) {
    match &tx.essence {
        TxEssence::Legacy(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price;
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_revm_b160(to_addr))
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.0.clone();
            tx_env.chain_id = tx.chain_id;
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list.clear();
        }
        TxEssence::Eip2930(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price;
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_revm_b160(to_addr))
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.0.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.clone().into();
        }
        TxEssence::Eip1559(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.max_fee_per_gas;
            tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas);
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_revm_b160(to_addr))
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.0.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.clone().into();
        }
    };
}
