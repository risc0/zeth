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

use std::{fmt::Debug, mem::take};

use anyhow::{anyhow, bail, Context, Result};
#[cfg(not(target_os = "zkvm"))]
use log::debug;
use revm::{
    primitives::{Address, ResultAndState, SpecId, TransactTo, TxEnv},
    Database, DatabaseCommit, EVM,
};
use ruint::aliases::U256;
use zeth_primitives::{
    receipt::Receipt,
    transactions::{
        ethereum::TransactionKind,
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        TxEssence,
    },
    trie::MptNode,
    Bloom, Bytes, RlpBytes,
};

use crate::{
    block_builder::BlockBuilder,
    consts,
    execution::{ethereum::fill_eth_tx_env, TxExecStrategy},
    guest_mem_forget,
};

/// Minimum supported protocol version: Bedrock (Block no. 105235063).
const MIN_SPEC_ID: SpecId = SpecId::BEDROCK;

pub struct OpTxExecStrategy {}

impl TxExecStrategy<OptimismTxEssence> for OpTxExecStrategy {
    fn execute_transactions<D>(
        mut block_builder: BlockBuilder<D, OptimismTxEssence>,
    ) -> Result<BlockBuilder<D, OptimismTxEssence>>
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
        let chain_id = block_builder.chain_spec.chain_id();

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
            info!("  Fee Recipient: {:?}", block_builder.input.beneficiary);
            info!("  Gas limit: {}", block_builder.input.gas_limit);
            info!("  Base fee per gas: {}", header.base_fee_per_gas);
            info!("  Extra data: {:?}", block_builder.input.extra_data);
        }

        // initialize the EVM
        let mut evm = EVM::new();

        // set the EVM configuration
        evm.env.cfg.chain_id = chain_id;
        evm.env.cfg.spec_id = spec_id;
        evm.env.cfg.optimism = true;

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
            // verify the transaction signature
            let tx_from = tx
                .recover_from()
                .with_context(|| format!("Error recovering address for transaction {}", tx_no))?;

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
                bail!("Error at transaction {}: gas exceeds block limit", tx_no);
            }

            match &tx.essence {
                OptimismTxEssence::OptimismDeposited(deposit) => {
                    #[cfg(not(target_os = "zkvm"))]
                    {
                        debug!("  Source: {:?}", &deposit.source_hash);
                        debug!("  Mint: {:?}", &deposit.mint);
                        debug!("  System Tx: {:?}", deposit.is_system_tx);
                    }

                    evm.env.tx.optimism.source_hash = Some(deposit.source_hash);
                    evm.env.tx.optimism.mint = Some(deposit.mint.try_into().unwrap());
                    evm.env.tx.optimism.is_system_transaction = Some(deposit.is_system_tx);
                    evm.env.tx.optimism.enveloped_tx = None; // only used for non-deposit txs

                    // Initialize tx environment
                    fill_deposit_tx_env(&mut evm.env.tx, deposit, tx_from);
                }
                OptimismTxEssence::Ethereum(transaction) => {
                    evm.env.tx.optimism.source_hash = None;
                    evm.env.tx.optimism.mint = None;
                    evm.env.tx.optimism.is_system_transaction = None;
                    evm.env.tx.optimism.enveloped_tx = Some(Bytes::from(tx.to_rlp()));

                    // Initialize tx environment
                    fill_eth_tx_env(&mut evm.env.tx, transaction, tx_from);
                }
            };

            // process the transaction
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))?;

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

            // update account states
            #[cfg(not(target_os = "zkvm"))]
            for (address, account) in &state {
                if account.is_touched() {
                    // log account
                    debug!(
                        "  State {:?} (is_selfdestructed={}, is_loaded_as_not_existing={}, is_created={})",
                        address,
                        account.is_selfdestructed(),
                        account.is_loaded_as_not_existing(),
                        account.is_created()
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

            let db = evm.db().unwrap();
            db.commit(state);

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(&receipt.payload.logs_bloom);

            // Add receipt and tx to tries
            let trie_key = tx_no.to_rlp();
            tx_trie
                .insert_rlp(&trie_key, tx)
                .context("failed to insert transaction")?;
            receipt_trie
                .insert_rlp(&trie_key, receipt)
                .context("failed to insert receipt")?;
        }

        // Update result header with computed values
        header.transactions_root = tx_trie.hash();
        header.receipts_root = receipt_trie.hash();
        header.logs_bloom = logs_bloom;
        header.gas_used = cumulative_gas_used;
        header.withdrawals_root = None;

        // Leak memory, save cycles
        guest_mem_forget([tx_trie, receipt_trie]);
        // Return block builder with updated database
        Ok(block_builder.with_db(evm.take_db()))
    }
}

fn fill_deposit_tx_env(tx_env: &mut TxEnv, tx: &TxEssenceOptimismDeposited, caller: Address) {
    tx_env.caller = caller; // previously overridden to tx.from
    tx_env.gas_limit = tx.gas_limit.try_into().unwrap();
    tx_env.gas_price = U256::ZERO;
    tx_env.gas_priority_fee = None;
    tx_env.transact_to = if let TransactionKind::Call(to_addr) = tx.to {
        TransactTo::Call(to_addr)
    } else {
        TransactTo::create()
    };
    tx_env.value = tx.value;
    tx_env.data = tx.data.clone();
    tx_env.chain_id = None;
    tx_env.nonce = None;
    tx_env.access_list.clear();
}
