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

use core::{fmt::Debug, mem::take, str::from_utf8};

use anyhow::{anyhow, bail, Context, Result};
#[cfg(not(target_os = "zkvm"))]
use log::debug;
use log::info;
use revm::{
    interpreter::Host,
    primitives::{Address, ResultAndState, SpecId, TxEnv},
    taiko, Database, DatabaseCommit, Evm,
};
use ruint::aliases::U256;
use zeth_primitives::{
    receipt::Receipt,
    transactions::{ethereum::EthereumTxEssence, TxEssence},
    trie::MptNode,
    Bloom, RlpBytes,
};

use super::{ethereum, TxExecStrategy};
use crate::{
    builder::BlockBuilder,
    consts::{self, ChainSpec},
    guest_mem_forget,
};

/// Minimum supported protocol version: Bedrock (Block no. 105235063).
const MIN_SPEC_ID: SpecId = SpecId::SHANGHAI /*change*/;

pub struct TkoTxExecStrategy {}

impl TxExecStrategy<EthereumTxEssence> for TkoTxExecStrategy {
    fn execute_transactions<D>(
        mut block_builder: BlockBuilder<D, EthereumTxEssence>,
    ) -> Result<BlockBuilder<D, EthereumTxEssence>>
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

        let mut evm = Evm::builder()
            .spec_id(spec_id)
            .modify_cfg_env(|cfg_env| {
                // set the EVM configuration
                cfg_env.chain_id = chain_id;
                cfg_env.taiko = true;
            })
            .modify_block_env(|blk_env| {
                // set the EVM block environment
                blk_env.number = header.number.try_into().unwrap();
                blk_env.coinbase = block_builder.input.beneficiary;
                blk_env.timestamp = header.timestamp;
                blk_env.difficulty = U256::ZERO;
                blk_env.prevrandao = Some(header.mix_hash);
                blk_env.basefee = header.base_fee_per_gas;
                blk_env.gas_limit = block_builder.input.gas_limit;
            })
            .with_db(block_builder.db.take().unwrap())
            .append_handler_register(taiko::handler_register::taiko_handle_register)
            .build();

        // bloom filter over all transaction logs
        let mut logs_bloom = Bloom::default();
        // keep track of the gas used over all transactions
        let mut cumulative_gas_used = consts::ZERO;

        // process all the transactions
        let mut tx_trie = MptNode::default();
        let mut receipt_trie = MptNode::default();
        let mut actual_tx_no = 0usize;

        for (tx_no, tx) in take(&mut block_builder.input.transactions)
            .into_iter()
            .enumerate()
        {
            // anchor transaction must be executed successfully
            let is_anchor = tx_no == 0;
            // verify the transaction signature
            let tx_from = tx
                .recover_from()
                .with_context(|| anyhow!("Error recovering address for transaction {}", tx_no))?;

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

            fill_eth_tx_env(
                block_builder.chain_spec,
                &mut evm.env().tx,
                &tx.essence,
                tx_from,
                is_anchor,
            );

            // process the transaction
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))?;

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

            actual_tx_no += 1;

            evm.context.evm.db.commit(state);

            // accumulate logs to the block bloom filter
            logs_bloom.accrue_bloom(&receipt.payload.logs_bloom);

            // Add receipt and tx to tries
            let trie_key = tx_no.to_rlp();
            tx_trie.insert_rlp(&trie_key, tx)?;
            receipt_trie.insert_rlp(&trie_key, receipt)?;
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
        Ok(block_builder.with_db(evm.context.evm.db))
    }
}

pub fn fill_eth_tx_env(
    _l2_chain_spec: &ChainSpec,
    tx_env: &mut TxEnv,
    essence: &EthereumTxEssence,
    caller: Address,
    is_anchor: bool,
) {
    // claim the anchor
    tx_env.taiko.is_anchor = is_anchor;
    // set the treasury address
    tx_env.taiko.treasury = *crate::taiko::consts::testnet::L2_CONTRACT;

    ethereum::fill_eth_tx_env(tx_env, essence, caller);
}
