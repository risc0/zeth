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

use std::{fmt::Debug, mem::take, str::from_utf8};

use anyhow::{anyhow, bail, Context};
#[cfg(all(not(target_os = "zkvm"), not(feature = "server")))]
use log::debug;
use revm::{
    primitives::{Account, Address, ResultAndState, SpecId, TransactTo, TxEnv, U256},
    Database, DatabaseCommit, EVM,
};
#[cfg(all(not(target_os = "zkvm"), feature = "server"))]
use tracing::debug;
use zeth_primitives::{
    receipt::Receipt,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        TxEssence,
    },
    trie::MptNode,
    Bloom, RlpBytes,
};

use crate::{
    block_builder::BlockBuilder,
    consts,
    consts::{ChainSpec, GWEI_TO_WEI, MIN_SPEC_ID},
    execution::TxExecStrategy,
    guest_mem_forget,
};

pub struct TaikoTxExecStrategy {}

impl TxExecStrategy<EthereumTxEssence> for TaikoTxExecStrategy {
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
            bail!(
                "Invalid protocol version: expected >= {:?}, got {:?}",
                MIN_SPEC_ID,
                spec_id,
            )
        }

        #[cfg(not(target_os = "zkvm"))]
        {
            use chrono::{TimeZone, Utc};
            use log::info;
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

        // set the EVM configuration
        evm.env.cfg.chain_id = block_builder.chain_spec.chain_id();
        evm.env.cfg.spec_id = spec_id;

        // set the EVM block environment
        evm.env.block.number = header.number.try_into().unwrap();
        evm.env.block.coinbase = block_builder.input.beneficiary;
        evm.env.block.timestamp = header.timestamp;
        evm.env.block.difficulty = U256::ZERO;
        evm.env.block.prevrandao = Some(header.mix_hash);
        evm.env.block.basefee = header.base_fee_per_gas;
        evm.env.block.gas_limit = block_builder.input.gas_limit;

        evm.database(block_builder.db.take().unwrap());

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = consts::ZERO;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in take(&mut block_builder.input.transactions)
            .into_iter()
            .enumerate()
        {
            // anchor transaction must be executed successfully
            let is_anchor = tx_no == 0;
            // verify the transaction signature
            let tx_from = match tx.recover_from() {
                Ok(tx_from) => tx_from,
                Err(err) => {
                    if is_anchor {
                        bail!("Error recovering anchor signature: {}", err);
                    }
                    #[cfg(not(target_os = "zkvm"))]
                    debug!(
                        "Error recovering address for transaction {}, error: {}",
                        tx_no, err
                    );
                    continue;
                }
            };

            #[cfg(not(target_os = "zkvm"))]
            {
                let tx_hash = tx.hash();
                debug!("Tx no. {} (hash: {})", tx_no, tx_hash);
                debug!("  Type: {}", tx.essence.tx_type());
                debug!("  Fr: {:?}", tx_from);
                debug!("  To: {:?}", tx.essence.to().unwrap_or_default());
            }

            // verify transaction gas
            let block_available_gas = block_builder.input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx.essence.gas_limit() {
                if is_anchor {
                    bail!("Error at transaction {}: gas exceeds block limit", tx_no);
                }
                #[cfg(not(target_os = "zkvm"))]
                debug!("Error at transaction {}: gas exceeds block limit", tx_no);
                continue;
            }

            // process the transaction
            fill_eth_tx_env(
                block_builder.chain_spec,
                &mut evm.env.tx,
                &tx.essence,
                tx_from,
                is_anchor,
            );
            let ResultAndState { result, state } = match evm.transact() {
                Ok(result) => result,
                Err(err) => {
                    if is_anchor {
                        bail!("Error at transaction {}: {:?}", tx_no, err);
                    }
                    #[cfg(not(target_os = "zkvm"))]
                    debug!("Error at transaction {}: {:?}", tx_no, err);
                    continue;
                }
            };

            if is_anchor && !result.is_success() {
                bail!(
                    "Error at transaction {}: execute anchor failed {:?}, output {:?}",
                    tx_no,
                    result,
                    result.output().map(|o| from_utf8(o).unwrap_or_default())
                );
            }

            let gas_used = result.gas_used().try_into().unwrap();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            #[cfg(not(target_os = "zkvm"))]
            debug!("  Ok: {:?}", result);

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
            let trie_key = tx_no.to_rlp();
            match tx_trie.insert_rlp(&trie_key, tx) {
                Ok(_) => (),
                Err(err) => {
                    if is_anchor {
                        bail!("Failed to insert transaction {}: {:?}", tx_no, err);
                    }
                    #[cfg(not(target_os = "zkvm"))]
                    debug!("Failed to insert transaction {}: {:?}", tx_no, err);
                    continue;
                }
            }
            match receipt_trie.insert_rlp(&trie_key, receipt) {
                Ok(_) => (),
                Err(err) => {
                    if is_anchor {
                        bail!("Failed to insert receipt {}: {:?}", tx_no, err);
                    }
                    #[cfg(not(target_os = "zkvm"))]
                    debug!("Failed to insert receipt {}: {:?}", tx_no, err);
                    continue;
                }
            }

            // update account states
            #[cfg(not(target_os = "zkvm"))]
            for (address, account) in &state {
                if account.is_touched() {
                    // log account
                    debug!(
                        "  State {:?} (is_selfdestructed={}, is_loaded_as_not_existing={}, is_created={}, is_empty={})",
                        address,
                        account.is_selfdestructed(),
                        account.is_loaded_as_not_existing(),
                        account.is_created(),
                        account.is_empty(),
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
                            debug!("      Before: {:?}", slot.original_value());
                            debug!("       After: {:?}", slot.present_value());
                        }
                    }
                }
            }

            evm.db().unwrap().commit(state);
        }

        let mut db = evm.take_db();

        // process withdrawals unconditionally after any transactions
        let mut withdrawals_trie = MptNode::default();
        for (i, withdrawal) in take(&mut block_builder.input.withdrawals)
            .into_iter()
            .enumerate()
        {
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
            // Credit withdrawal amount
            increase_account_balance(&mut db, withdrawal.address, amount_wei)?;
            // Add withdrawal to trie
            withdrawals_trie
                .insert_rlp(&i.to_rlp(), withdrawal)
                .with_context(|| "failed to insert withdrawal")?;
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

pub fn fill_eth_tx_env(
    l2_chain_spec: &ChainSpec,
    tx_env: &mut TxEnv,
    essence: &EthereumTxEssence,
    caller: Address,
    is_anchor: bool,
) {
    // claim the anchor
    tx_env.taiko.is_anchor = is_anchor;
    // set the treasury address
    tx_env.taiko.treasury = l2_chain_spec.l2_contract.unwrap();
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
