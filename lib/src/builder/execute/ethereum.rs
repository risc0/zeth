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

use core::{fmt::Debug, mem::take};

use anyhow::{anyhow, bail, Context};
#[cfg(not(target_os = "zkvm"))]
use log::{debug, trace};
use revm::{
    interpreter::Host,
    primitives::{Account, Address, ResultAndState, SpecId, TransactTo, TxEnv},
    Database, DatabaseCommit, Evm,
};
use ruint::aliases::U256;
use zeth_primitives::{
    alloy_rlp,
    receipt::Receipt,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        TxEssence,
    },
    trie::MptNode,
    Bloom,
};

use super::TxExecStrategy;
use crate::{builder::BlockBuilder, consts, guest_mem_forget};

/// Minimum supported protocol version: Paris (Block no. 15537394).
const MIN_SPEC_ID: SpecId = SpecId::MERGE;

pub struct EthTxExecStrategy {}

impl TxExecStrategy<EthereumTxEssence> for EthTxExecStrategy {
    fn execute_transactions<D>(
        mut block_builder: BlockBuilder<D, EthereumTxEssence>,
    ) -> anyhow::Result<BlockBuilder<D, EthereumTxEssence>>
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
            panic!(
                "Invalid protocol version: expected >= {:?}, got {:?}",
                MIN_SPEC_ID, spec_id,
            )
        }

        #[cfg(not(target_os = "zkvm"))]
        {
            use chrono::{TimeZone, Utc};
            let dt = Utc
                .timestamp_opt(
                    block_builder
                        .input
                        .state_input
                        .timestamp
                        .try_into()
                        .unwrap(),
                    0,
                )
                .unwrap();

            debug!("Block no. {}", header.number);
            debug!("  EVM spec ID: {:?}", spec_id);
            debug!("  Timestamp: {}", dt);
            trace!(
                "  Transactions: {}",
                block_builder.input.state_input.transactions.len()
            );
            trace!(
                "  Withdrawals: {}",
                block_builder.input.state_input.withdrawals.len()
            );
            trace!(
                "  Fee Recipient: {:?}",
                block_builder.input.state_input.beneficiary
            );
            trace!("  Gas limit: {}", block_builder.input.state_input.gas_limit);
            trace!("  Base fee per gas: {}", header.base_fee_per_gas);
            trace!(
                "  Extra data: {:?}",
                block_builder.input.state_input.extra_data
            );
        }

        // initialize the Evm
        let mut evm = Evm::builder()
            .with_db(block_builder.db.take().unwrap())
            .with_spec_id(spec_id)
            .modify_block_env(|blk_env| {
                // set the EVM block environment
                blk_env.number = header.number.try_into().unwrap();
                blk_env.coinbase = block_builder.input.state_input.beneficiary;
                blk_env.timestamp = header.timestamp;
                blk_env.difficulty = U256::ZERO;
                blk_env.prevrandao = Some(header.mix_hash);
                blk_env.basefee = header.base_fee_per_gas;
                blk_env.gas_limit = block_builder.input.state_input.gas_limit;
            })
            .modify_cfg_env(|cfg_env| {
                // set the EVM configuration
                cfg_env.chain_id = block_builder.chain_spec.chain_id();
            })
            .build();

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = consts::ZERO;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in take(&mut block_builder.input.state_input.transactions)
            .into_iter()
            .enumerate()
        {
            // verify the transaction signature
            let tx_from = tx
                .recover_from()
                .with_context(|| format!("Error recovering address for transaction {}", tx_no))?;

            #[cfg(not(target_os = "zkvm"))]
            {
                let tx_hash = tx.hash();
                trace!("Tx no. {} (hash: {})", tx_no, tx_hash);
                trace!("  Type: {}", tx.essence.tx_type());
                trace!("  Fr: {:?}", tx_from);
                trace!("  To: {:?}", tx.essence.to().unwrap_or_default());
            }

            // verify transaction gas
            let block_available_gas =
                block_builder.input.state_input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx.essence.gas_limit() {
                bail!("Error at transaction {}: gas exceeds block limit", tx_no);
            }

            // process the transaction
            fill_eth_tx_env(&mut evm.env_mut().tx, &tx.essence, tx_from);
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("Block construction failure.");

            let gas_used = result.gas_used().try_into().unwrap();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            #[cfg(not(target_os = "zkvm"))]
            trace!("  Ok: {:?}", result);

            // create the receipt from the EVM result
            let receipt = Receipt::new(
                tx.essence.tx_type(),
                result.is_success(),
                cumulative_gas_used,
                result.logs().into_iter().map(|log| log.into()).collect(),
            );

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(&receipt.payload.logs_bloom);

            // Add receipt and tx to tries
            let trie_key = alloy_rlp::encode(tx_no);
            tx_trie
                .insert_rlp(&trie_key, tx)
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("failed to insert transaction");
            receipt_trie
                .insert_rlp(&trie_key, receipt)
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("failed to insert receipt");

            // update account states
            #[cfg(not(target_os = "zkvm"))]
            for (address, account) in &state {
                if account.is_touched() {
                    // log account
                    trace!(
                        "  State {:?} (is_selfdestructed={}, is_loaded_as_not_existing={}, is_created={}, is_empty={})",
                        address,
                        account.is_selfdestructed(),
                        account.is_loaded_as_not_existing(),
                        account.is_created(),
                        account.is_empty(),
                    );
                    // log balance changes
                    trace!(
                        "     After balance: {} (Nonce: {})",
                        account.info.balance,
                        account.info.nonce
                    );

                    // log state changes
                    for (addr, slot) in &account.storage {
                        if slot.is_changed() {
                            trace!("    Storage address: {:?}", addr);
                            trace!("      Before: {:?}", slot.original_value());
                            trace!("       After: {:?}", slot.present_value());
                        }
                    }
                }
            }

            evm.context.evm.db.commit(state);
        }

        // process withdrawals unconditionally after any transactions
        let mut withdrawals_trie = MptNode::default();
        for (i, withdrawal) in take(&mut block_builder.input.state_input.withdrawals)
            .into_iter()
            .enumerate()
        {
            // the withdrawal amount is given in Gwei
            let amount_wei = consts::GWEI_TO_WEI
                .checked_mul(withdrawal.amount.try_into().unwrap())
                .unwrap();

            #[cfg(not(target_os = "zkvm"))]
            {
                trace!("Withdrawal no. {}", withdrawal.index);
                trace!("  Recipient: {:?}", withdrawal.address);
                trace!("  Value: {}", amount_wei);
            }
            // Credit withdrawal amount
            increase_account_balance(&mut evm.context.evm.db, withdrawal.address, amount_wei)
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("Failed to increase account balance.");
            // Add withdrawal to trie
            withdrawals_trie
                .insert_rlp(&alloy_rlp::encode(i), withdrawal)
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("failed to insert withdrawal");
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
        Ok(block_builder.with_db(evm.context.evm.db))
    }
}

pub fn fill_eth_tx_env(tx_env: &mut TxEnv, essence: &EthereumTxEssence, caller: Address) {
    match essence {
        EthereumTxEssence::Legacy(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price;
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.clone();
            tx_env.chain_id = tx.chain_id;
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list.clear();
        }
        EthereumTxEssence::Eip2930(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.gas_price;
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.clone().into();
        }
        EthereumTxEssence::Eip1559(tx) => {
            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
            tx_env.gas_price = tx.max_fee_per_gas;
            tx_env.gas_priority_fee = Some(tx.max_priority_fee_per_gas);
            tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.data.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.clone().into();
        }
    };
}

pub fn increase_account_balance<D>(
    db: &mut D,
    address: Address,
    amount_wei: U256,
) -> anyhow::Result<()>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: Debug,
{
    // Read account from database
    let mut account: Account = db
        .basic(address)
        .map_err(|db_err| {
            anyhow!(
                "Error increasing account balance for {}: {:?}",
                address,
                db_err
            )
        })?
        .unwrap_or_default()
        .into();
    // Credit withdrawal amount
    account.info.balance = account.info.balance.checked_add(amount_wei).unwrap();
    account.mark_touch();
    // Commit changes to database
    db.commit([(address, account)].into());

    Ok(())
}
