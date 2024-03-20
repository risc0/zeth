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

use anyhow::{anyhow, bail, ensure, Context};
#[cfg(not(target_os = "zkvm"))]
use log::{debug, trace};
use revm::{
    interpreter::Host,
    primitives::{Account, ResultAndState, SpecId, TransactTo, TxEnv, MAX_BLOB_GAS_PER_BLOCK},
    Database, DatabaseCommit, Evm,
};
use ruint::aliases::U256;
use zeth_primitives::{
    alloy_rlp,
    receipt::{EthReceiptEnvelope, Receipt, ReceiptEnvelope},
    transactions::{EthTxEnvelope, EvmTransaction, TxEnvelope, TxType},
    trie::MptNode,
    Address, Bloom,
};

use super::TxExecStrategy;
use crate::{
    builder::BlockBuilder,
    consts::{self, BEACON_ROOTS_ADDRESS, SYSTEM_ADDRESS},
    guest_mem_forget,
};

pub struct EthTxExecStrategy {}

impl TxExecStrategy for EthTxExecStrategy {
    fn execute_transactions<D>(
        mut block_builder: BlockBuilder<D>,
    ) -> anyhow::Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug,
    {
        let spec_id = block_builder.spec_id.expect("Spec ID is not initialized");
        let header = block_builder
            .header
            .as_mut()
            .expect("Header is not initialized");

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
                        .unwrap_or(32503676400),
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
                blk_env.timestamp = U256::from(header.timestamp);
                blk_env.gas_limit = U256::from(block_builder.input.state_input.gas_limit);
                blk_env.basefee = U256::from(header.base_fee_per_gas.unwrap_or_default());
                blk_env.difficulty = U256::ZERO;
                blk_env.prevrandao = Some(header.mix_hash);
                // EIP-4844 excess blob gas of this block, introduced in Cancun
                if let Some(excess_blob_gas) = header.excess_blob_gas {
                    blk_env.set_blob_excess_gas_and_price(excess_blob_gas)
                }
            })
            .modify_cfg_env(|cfg_env| {
                // set the EVM configuration
                cfg_env.chain_id = block_builder.chain_spec.chain_id();
            })
            .build();

        // set the beacon block root in the EVM
        if spec_id >= SpecId::CANCUN {
            let parent_beacon_block_root = header.parent_beacon_block_root.unwrap();

            // From EIP-4788 Beacon block root in the EVM (Cancun):
            // "Call BEACON_ROOTS_ADDRESS as SYSTEM_ADDRESS with the 32-byte input of
            //  header.parent_beacon_block_root, a gas limit of 30_000_000, and 0 value."
            evm.env_mut().tx = TxEnv {
                transact_to: TransactTo::Call(BEACON_ROOTS_ADDRESS),
                caller: SYSTEM_ADDRESS,
                data: parent_beacon_block_root.0.into(),
                gas_limit: 30_000_000,
                value: U256::ZERO,
                ..Default::default()
            };

            let tmp = evm.env_mut().block.clone();

            // disable block gas limit validation and base fee checks
            evm.block_mut().gas_limit = U256::from(evm.tx().gas_limit);
            evm.block_mut().basefee = U256::ZERO;

            let ResultAndState { mut state, .. } =
                evm.transact().expect("beacon roots contract call failed");
            evm.env_mut().block = tmp;

            // commit only the changes to the beacon roots contract
            state.remove(&SYSTEM_ADDRESS);
            state.remove(&evm.block().coinbase);
            evm.context.evm.db.commit(state);
        }

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = 0_u64;

        let mut blob_gas_used = 0_u64;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in take(&mut block_builder.input.state_input.transactions)
            .into_iter()
            .enumerate()
        {
            // verify the transaction signature
            let tx_from: Address = tx
                .from()
                .with_context(|| format!("Error recovering address for transaction {}", tx_no))?;

            #[cfg(not(target_os = "zkvm"))]
            {
                let tx_hash = tx.hash();
                trace!("Tx no. {} (hash: {})", tx_no, tx_hash);
                trace!("  Type: {:?}", tx.tx_type());
                trace!("  Fr: {:?}", tx_from);
                trace!("  To: {:?}", tx.to().to().unwrap_or_default());
            }

            // validate transaction gas
            let block_available_gas =
                block_builder.input.state_input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx.gas_limit() {
                bail!("Error at transaction {}: gas exceeds block limit", tx_no);
            }

            // validity blob gas
            if let TxEnvelope::Ethereum(EthTxEnvelope::Eip4844(blob_tx)) = &tx {
                let tx = blob_tx.tx().tx();
                blob_gas_used = blob_gas_used.checked_add(tx.blob_gas()).unwrap();
                ensure!(
                    blob_gas_used <= MAX_BLOB_GAS_PER_BLOCK,
                    "Error at transaction {}: total blob gas spent exceeds the limit",
                    tx_no
                );
            }

            let TxEnvelope::Ethereum(essence) = &tx else {
                unreachable!("OptimismDeposit transactions are not supported")
            };

            // process the transaction
            fill_eth_tx_env(&mut evm.env_mut().tx, essence, tx_from);
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("Block construction failure");

            let gas_used = result.gas_used();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            #[cfg(not(target_os = "zkvm"))]
            trace!("  Ok: {:?}", result);

            // create the receipt from the EVM result
            let receipt = Receipt {
                success: result.is_success(),
                cumulative_gas_used,
                logs: result.into_logs(),
            }
            .with_bloom();

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(&receipt.bloom);

            // create the EIP-2718 enveloped receipt
            let receipt = match tx.tx_type() {
                TxType::Legacy => ReceiptEnvelope::Ethereum(EthReceiptEnvelope::Legacy(receipt)),
                TxType::Eip2930 => ReceiptEnvelope::Ethereum(EthReceiptEnvelope::Eip2930(receipt)),
                TxType::Eip1559 => ReceiptEnvelope::Ethereum(EthReceiptEnvelope::Eip1559(receipt)),
                TxType::Eip4844 => ReceiptEnvelope::Ethereum(EthReceiptEnvelope::Eip4844(receipt)),
                TxType::OptimismDeposit => unreachable!(),
            };

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
        if spec_id >= SpecId::SHANGHAI {
            header.withdrawals_root = Some(withdrawals_trie.hash());
        }
        if spec_id >= SpecId::CANCUN {
            header.blob_gas_used = Some(blob_gas_used);
        }

        // Leak memory, save cycles
        guest_mem_forget([tx_trie, receipt_trie, withdrawals_trie]);
        // Return block builder with updated database
        Ok(block_builder.with_db(evm.context.evm.inner.db))
    }
}

pub fn fill_eth_tx_env(tx_env: &mut TxEnv, essence: &EthTxEnvelope, caller: Address) {
    match essence {
        EthTxEnvelope::Legacy(tx) => {
            let tx = tx.tx();

            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit;
            tx_env.gas_price = U256::from(tx.gas_price);
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let Some(to_addr) = tx.to.to() {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = tx.chain_id;
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list.clear();
            tx_env.blob_hashes.clear();
            tx_env.max_fee_per_blob_gas.take();
        }
        EthTxEnvelope::Eip2930(tx) => {
            let tx = tx.tx();

            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit;
            tx_env.gas_price = U256::from(tx.gas_price);
            tx_env.gas_priority_fee = None;
            tx_env.transact_to = if let Some(to_addr) = tx.to.to() {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
            tx_env.blob_hashes.clear();
            tx_env.max_fee_per_blob_gas.take();
        }
        EthTxEnvelope::Eip1559(tx) => {
            let tx = tx.tx();

            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit;
            tx_env.gas_price = U256::from(tx.max_fee_per_gas);
            tx_env.gas_priority_fee = Some(U256::from(tx.max_priority_fee_per_gas));
            tx_env.transact_to = if let Some(to_addr) = tx.to.to() {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
            tx_env.blob_hashes.clear();
            tx_env.max_fee_per_blob_gas.take();
        }
        EthTxEnvelope::Eip4844(tx) => {
            let tx = tx.tx().tx();

            tx_env.caller = caller;
            tx_env.gas_limit = tx.gas_limit;
            tx_env.gas_price = U256::from(tx.max_fee_per_gas);
            tx_env.gas_priority_fee = Some(U256::from(tx.max_priority_fee_per_gas));
            tx_env.transact_to = if let Some(to_addr) = tx.to.to() {
                TransactTo::Call(to_addr)
            } else {
                TransactTo::create()
            };
            tx_env.value = tx.value;
            tx_env.data = tx.input.clone();
            tx_env.chain_id = Some(tx.chain_id);
            tx_env.nonce = Some(tx.nonce);
            tx_env.access_list = tx.access_list.flattened();
            tx_env.blob_hashes = tx.blob_versioned_hashes.clone();
            tx_env.max_fee_per_blob_gas = Some(U256::from(tx.max_fee_per_blob_gas));
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
