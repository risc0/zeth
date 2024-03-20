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

use anyhow::{anyhow, bail, Context, Result};
#[cfg(not(target_os = "zkvm"))]
use log::{debug, trace};
use revm::{
    interpreter::Host,
    primitives::{Address, ResultAndState, SpecId, TransactTo, TxEnv},
    Database, DatabaseCommit, Evm,
};
use ruint::aliases::U256;
use zeth_primitives::{
    alloy_rlp,
    receipt::{EthReceiptEnvelope, OptimismDepositReceipt, Receipt, ReceiptEnvelope},
    transactions::{
        optimism::TxOptimismDeposit, EthTxEnvelope, EvmTransaction as _, TxEnvelope, TxType,
    },
    trie::{MptNode, EMPTY_ROOT},
    Bloom, Bytes,
};

use super::{ethereum, TxExecStrategy};
use crate::{builder::BlockBuilder, guest_mem_forget};

pub struct OpTxExecStrategy {}

impl TxExecStrategy for OpTxExecStrategy {
    fn execute_transactions<D>(mut block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
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
                "  Fee Recipient: {:?}",
                block_builder.input.state_input.beneficiary
            );
            trace!("  Gas limit: {}", block_builder.input.state_input.gas_limit);
            trace!(
                "  Extra data: {:?}",
                block_builder.input.state_input.extra_data
            );
        }

        let chain_id = block_builder.chain_spec.chain_id();
        let mut evm = Evm::builder()
            .with_db(block_builder.db.take().unwrap())
            .optimism()
            .with_spec_id(spec_id)
            .modify_block_env(|blk_env| {
                // set the EVM block environment
                blk_env.number = header.number.try_into().unwrap();
                blk_env.coinbase = block_builder.input.state_input.beneficiary;
                blk_env.timestamp = U256::from(header.timestamp);
                blk_env.difficulty = U256::ZERO;
                blk_env.prevrandao = Some(header.mix_hash);
                blk_env.basefee = U256::from(header.base_fee_per_gas.unwrap_or_default());
                blk_env.gas_limit = U256::from(block_builder.input.state_input.gas_limit);
            })
            .modify_cfg_env(|cfg_env| {
                // set the EVM configuration
                cfg_env.chain_id = chain_id;
            })
            .build();

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = 0_u64;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        for (tx_no, tx) in take(&mut block_builder.input.state_input.transactions)
            .into_iter()
            .enumerate()
        {
            // verify the transaction signature
            let tx_from = tx
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

            // verify transaction gas
            let block_available_gas =
                block_builder.input.state_input.gas_limit - cumulative_gas_used;
            if block_available_gas < tx.gas_limit() {
                bail!("Error at transaction {}: gas exceeds block limit", tx_no);
            }

            // cache account nonce if the transaction is a deposit, starting with Canyon
            let deposit_nonce = (spec_id >= SpecId::CANYON
                && matches!(tx, TxEnvelope::OptimismDeposit(_)))
            .then(|| {
                let db = &mut evm.context.evm.db;
                let account = db.basic(tx_from).expect("Depositor account not found");
                account.unwrap_or_default().nonce
            });

            match &tx {
                TxEnvelope::OptimismDeposit(op) => {
                    #[cfg(not(target_os = "zkvm"))]
                    {
                        trace!("  Source: {:?}", &op.source_hash);
                        trace!("  Mint: {:?}", &op.mint);
                        trace!("  System Tx: {:?}", op.is_system_tx);
                    }

                    // Initialize tx environment
                    fill_deposit_tx_env(&mut evm.env_mut().tx, op, tx_from);
                }
                TxEnvelope::Ethereum(eth) => {
                    fill_eth_tx_env(&mut evm.env_mut().tx, alloy_rlp::encode(&tx), eth, tx_from);
                }
            };

            // process the transaction
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))
                // todo: change unrecoverable panic to host-side recoverable `Result`
                .expect("Block construction failure");

            cumulative_gas_used = cumulative_gas_used.checked_add(result.gas_used()).unwrap();

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
                TxType::OptimismDeposit => ReceiptEnvelope::OptimismDeposit(
                    OptimismDepositReceipt::new(receipt, deposit_nonce),
                ),
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

        // Update result header with computed values
        header.transactions_root = tx_trie.hash();
        header.receipts_root = receipt_trie.hash();
        header.logs_bloom = logs_bloom;
        header.gas_used = cumulative_gas_used;
        header.withdrawals_root = if spec_id < SpecId::CANYON {
            None
        } else {
            Some(EMPTY_ROOT)
        };

        // Leak memory, save cycles
        guest_mem_forget([tx_trie, receipt_trie]);
        // Return block builder with updated database
        Ok(block_builder.with_db(evm.context.evm.inner.db))
    }
}

fn fill_deposit_tx_env(tx_env: &mut TxEnv, tx: &TxOptimismDeposit, caller: Address) {
    // initialize additional optimism tx fields
    tx_env.optimism.source_hash = Some(tx.source_hash);
    tx_env.optimism.mint = Some(tx.mint.try_into().unwrap());
    tx_env.optimism.is_system_transaction = Some(tx.is_system_tx);
    tx_env.optimism.enveloped_tx = None; // only used for non-deposit txs

    tx_env.caller = caller; // previously overridden to tx.from
    tx_env.gas_limit = tx.gas_limit;
    tx_env.gas_price = U256::ZERO;
    tx_env.gas_priority_fee = None;
    tx_env.transact_to = if let Some(to_addr) = tx.to.to() {
        TransactTo::Call(to_addr)
    } else {
        TransactTo::create()
    };
    tx_env.value = tx.value;
    tx_env.data = tx.input.clone();
    tx_env.chain_id = None;
    tx_env.nonce = None;
    tx_env.access_list.clear();
}

fn fill_eth_tx_env(tx_env: &mut TxEnv, tx: Vec<u8>, essence: &EthTxEnvelope, caller: Address) {
    // initialize additional optimism tx fields
    tx_env.optimism.source_hash = None;
    tx_env.optimism.mint = None;
    tx_env.optimism.is_system_transaction = Some(false);
    tx_env.optimism.enveloped_tx = Some(Bytes::from(tx));

    ethereum::fill_eth_tx_env(tx_env, essence, caller);
}
