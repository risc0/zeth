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

use core::fmt::Debug;

use anyhow::{anyhow, bail, Context, Result};
#[cfg(not(target_os = "zkvm"))]
use log::{debug, info};
use revm::{
    primitives::{
        Account, Address, BlockEnv, CfgEnv, ResultAndState, SpecId, TransactTo, TxEnv, U256,
    },
    Database, DatabaseCommit, EVM,
};
use zeth_primitives::{
    receipt::Receipt,
    revm::{to_revm_b160, to_revm_b256},
    transaction::{Transaction, TransactionKind, TxEssence},
    trie::MptNode,
    Bloom, RlpBytes,
};

use crate::{
    block_builder::BlockBuilder,
    consts,
    consts::{GWEI_TO_WEI, MIN_SPEC_ID},
    guest_mem_forget,
};

pub trait TxExecStrategy {
    fn execute_transactions<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug;
}

pub struct EthTxExecStrategy {}

impl TxExecStrategy for EthTxExecStrategy {
    fn execute_transactions<D>(mut block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
    {
        let header = block_builder
            .header
            .as_mut()
            .expect("Header is not initialized");
        // Compute the spec id
        let spec_id = block_builder.chain_spec.spec_id(header.number);
        if !SpecId::enabled(spec_id, MIN_SPEC_ID) {
            bail!(
                "Invalid protocol version: expected >= {:?}, got {:?}",
                MIN_SPEC_ID,
                spec_id,
            )
        }

        #[cfg(not(target_os = "zkvm"))]
        {
            use chrono::{TimeZone, Utc};
            let dt = Utc
                .timestamp_opt(block_builder.input.timestamp.try_into().unwrap(), 0)
                .unwrap();

            info!("Block no. {}", header.number);
            info!("  EVM spec ID: {:?}", spec_id);
            info!("  Timestamp: {}", dt);
            info!("  Transactions: {}", block_builder.input.transactions.len());
            info!("  Withdrawals: {}", block_builder.input.withdrawals.len());
            info!("  Fee Recipient: {:?}", block_builder.input.beneficiary);
            info!("  Gas limit: {}", block_builder.input.gas_limit);
            info!("  Base fee per gas: {}", header.base_fee_per_gas);
            info!("  Extra data: {:?}", block_builder.input.extra_data);
        }

        // initialize the EVM
        let mut evm = EVM::new();

        evm.env.cfg = CfgEnv {
            chain_id: U256::from(block_builder.chain_spec.chain_id()),
            spec_id,
            ..Default::default()
        };
        evm.env.block = BlockEnv {
            number: header.number.try_into().unwrap(),
            coinbase: to_revm_b160(block_builder.input.beneficiary),
            timestamp: block_builder.input.timestamp,
            difficulty: U256::ZERO,
            prevrandao: Some(to_revm_b256(block_builder.input.mix_hash)),
            basefee: header.base_fee_per_gas,
            gas_limit: block_builder.input.gas_limit,
        };

        evm.database(block_builder.db.take().unwrap());

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = consts::ZERO;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in block_builder.input.transactions.iter().enumerate() {
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
            let block_available_gas = block_builder.input.gas_limit - cumulative_gas_used;
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
            #[cfg(not(target_os = "zkvm"))]
            for (address, account) in &state {
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
            }

            evm.db().unwrap().commit(state);
        }

        let mut db = evm.take_db();

        // process withdrawals unconditionally after any transactions
        let mut withdrawals_trie = MptNode::default();
        for (i, withdrawal) in block_builder.input.withdrawals.iter().enumerate() {
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
            // Read account from database
            let withdrawal_address = to_revm_b160(withdrawal.address);
            let mut withdrawal_account: Account = db
                .basic(withdrawal_address)
                .map_err(|db_err| anyhow!("Error at withdrawal {}: {:?}", i, db_err))?
                .unwrap_or_default()
                .into();
            // Credit withdrawal amount
            withdrawal_account.info.balance = withdrawal_account
                .info
                .balance
                .checked_add(amount_wei)
                .unwrap();
            withdrawal_account.is_touched = true;
            // Commit changes to database
            db.commit([(withdrawal_address, withdrawal_account)].into());
            // Add withdrawal to trie
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
        // Return block builder with updated database
        Ok(block_builder.with_db(db))
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
