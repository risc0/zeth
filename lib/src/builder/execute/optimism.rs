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

use std::{fmt::Debug, mem::take, str::FromStr};

use anyhow::{anyhow, bail, Context, Result};
#[cfg(feature = "host")]
use log::debug;
use revm::{
    primitives::{Account, Address, ResultAndState, SpecId, TransactTo, TxEnv},
    Database, DatabaseCommit, EVM,
};
use ruint::aliases::U256;
use zeth_primitives::{
    receipt::Receipt,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        TxEssence,
    },
    trie::MptNode,
    Bloom, RlpBytes,
};

use super::{
    ethereum::{fill_eth_tx_env, increase_account_balance},
    TxExecStrategy,
};
use crate::{
    builder::BlockBuilder,
    consts,
    consts::{GWEI_TO_WEI, MIN_SPEC_ID},
    guest_mem_forget,
};
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

        #[cfg(feature = "host")]
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
        evm.env.cfg.chain_id = chain_id;
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

        let l1_info_depositor_address =
            Address::from_str("0xDeaDDEaDDeAdDeAdDEAdDEaddeAddEAdDEAd0001").unwrap();

        let l1_block_attr_pre_deploy =
            Address::from_str("0x4200000000000000000000000000000000000015").unwrap();

        let l1_gas_oracle_pre_deploy =
            Address::from_str("0x420000000000000000000000000000000000000F").unwrap();

        let l1_fee_vault_pre_deploy =
            Address::from_str("0x420000000000000000000000000000000000001A").unwrap();

        let base_fee_vault_pre_deploy =
            Address::from_str("0x4200000000000000000000000000000000000019").unwrap();

        let l1_fee_overhead_decimals = U256::from(10).pow(read_uint(
            &mut evm,
            vec![0x31u8, 0x3cu8, 0xe5u8, 0x67u8],
            Some(chain_id),
            header.gas_limit,
            l1_gas_oracle_pre_deploy,
        )?);

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

            #[cfg(feature = "host")]
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

            let l1_gas_fees = match &tx.essence {
                OptimismTxEssence::OptimismDeposited(deposit) => {
                    // Disable gas fees
                    evm.env.cfg.disable_base_fee = true;
                    evm.env.cfg.disable_balance_check = true;
                    // Irrevocably credit minted amount
                    let db = evm.db().unwrap();
                    increase_account_balance(db, tx_from, deposit.mint)?;
                    // Retrieve effective nonce
                    // todo: read this from contract
                    let effective_nonce = if tx_from == l1_info_depositor_address {
                        None
                    } else {
                        Some(db.basic(tx_from).unwrap().unwrap_or_default().nonce)
                    };
                    // Initialize tx environment
                    fill_deposit_tx_env(&mut evm.env.tx, deposit, tx_from, effective_nonce);

                    U256::ZERO
                }
                OptimismTxEssence::Ethereum(transaction) => {
                    // L1 gas fee
                    // todo: read these values only once after processing the first system tx
                    let l1_base_fee = read_uint(
                        &mut evm,
                        vec![0x5cu8, 0xf2u8, 0x49u8, 0x69u8],
                        Some(chain_id),
                        header.gas_limit,
                        l1_block_attr_pre_deploy,
                    )?;
                    let l1_fee_overhead = read_uint(
                        &mut evm,
                        vec![0x8bu8, 0x23u8, 0x9fu8, 0x73u8],
                        Some(chain_id),
                        header.gas_limit,
                        l1_block_attr_pre_deploy,
                    )?;
                    let l1_fee_scalar = read_uint(
                        &mut evm,
                        vec![0x9eu8, 0x8cu8, 0x49u8, 0x66u8],
                        Some(chain_id),
                        header.gas_limit,
                        l1_block_attr_pre_deploy,
                    )?;

                    let tx_data = tx.to_rlp();
                    let non_zero = tx_data.iter().filter(|b| *b > &0u8).count() as u128;
                    let zeroes = tx_data.len() as u128 - non_zero;
                    let l1_gas = U256::from(16u128 * non_zero + 4u128 * zeroes) + l1_fee_overhead;
                    let l1_gas_fees =
                        (l1_base_fee * l1_gas * l1_fee_scalar) / l1_fee_overhead_decimals;

                    // Deduct L1 fee from sender
                    decrease_account_balance(evm.db().unwrap(), tx_from, l1_gas_fees)?;

                    // Enable gas fees
                    evm.env.cfg.disable_base_fee = false;
                    evm.env.cfg.disable_balance_check = false;
                    // Initialize tx environment
                    fill_eth_tx_env(&mut evm.env.tx, transaction, tx_from);
                    l1_gas_fees
                }
            };

            // process the transaction
            let ResultAndState { result, state } = evm
                .transact()
                .map_err(|evm_err| anyhow!("Error at transaction {}: {:?}", tx_no, evm_err))?;

            let gas_used = result.gas_used().try_into().unwrap();
            cumulative_gas_used = cumulative_gas_used.checked_add(gas_used).unwrap();

            #[cfg(feature = "host")]
            debug!("  Ok: {:?}", result);

            // create the receipt from the EVM result
            let receipt = Receipt::new(
                tx.essence.tx_type(),
                result.is_success(),
                cumulative_gas_used,
                result.logs().into_iter().map(|log| log.into()).collect(),
            );

            // update account states
            #[cfg(feature = "host")]
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

            if !matches!(tx.essence, OptimismTxEssence::OptimismDeposited(_)) {
                // Credit L2 base fee
                increase_account_balance(
                    db,
                    base_fee_vault_pre_deploy,
                    gas_used * header.base_fee_per_gas,
                )?;
                // Credit L1 gas fee
                increase_account_balance(db, l1_fee_vault_pre_deploy, l1_gas_fees)?;
            }

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

            #[cfg(feature = "host")]
            {
                debug!("Withdrawal no. {}", withdrawal.index);
                debug!("  Recipient: {:?}", withdrawal.address);
                debug!("  Value: {}", amount_wei);
            }
            // Read account from database
            let withdrawal_address = withdrawal.address;
            let mut withdrawal_account: Account = db
                .basic(withdrawal.address)
                .map_err(|db_err| anyhow!("Error at withdrawal {}: {:?}", i, db_err))?
                .unwrap_or_default()
                .into();
            // Credit withdrawal amount
            withdrawal_account.info.balance = withdrawal_account
                .info
                .balance
                .checked_add(amount_wei)
                .unwrap();
            withdrawal_account.mark_touch();
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

fn read_uint<D>(
    evm: &mut EVM<D>,
    abi_call: Vec<u8>,
    chain_id: Option<zeth_primitives::ChainId>,
    gas_limit: U256,
    address: Address,
) -> Result<U256>
where
    D: Database + DatabaseCommit,
    <D as Database>::Error: Debug,
{
    let op_l1_tx =
        EthereumTxEssence::Legacy(zeth_primitives::transactions::ethereum::TxEssenceLegacy {
            chain_id,
            nonce: 0,
            gas_price: U256::ZERO,
            gas_limit,
            to: TransactionKind::Call(address),
            value: U256::ZERO,
            data: abi_call.into(),
        });

    // disable base fees
    evm.env.cfg.disable_base_fee = true;
    evm.env.cfg.disable_balance_check = true;
    fill_eth_tx_env(&mut evm.env.tx, &op_l1_tx, Default::default());

    let Ok(ResultAndState {
        result: execution_result,
        ..
    }) = evm.transact()
    else {
        bail!("Error during execution");
    };

    let revm::primitives::ExecutionResult::Success { output, .. } = execution_result else {
        bail!("Result unsuccessful");
    };

    let revm::primitives::Output::Call(result_encoded) = output else {
        bail!("Unsupported result");
    };

    let ethers_core::abi::Token::Uint(uint_result) =
        ethers_core::abi::decode(&[ethers_core::abi::ParamType::Uint(256)], &result_encoded)?
            .pop()
            .unwrap()
    else {
        bail!("Could not decode result");
    };

    Ok(U256::from_limbs(uint_result.0))
}

fn fill_deposit_tx_env(
    tx_env: &mut TxEnv,
    tx: &TxEssenceOptimismDeposited,
    caller: Address,
    deposit_nonce: Option<u64>,
) {
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
    tx_env.nonce = deposit_nonce;
    tx_env.access_list.clear();
}

pub fn decrease_account_balance<D>(
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
                "Error decreasing account balance for {}: {:?}",
                address,
                db_err
            )
        })?
        .unwrap_or_default()
        .into();
    // Credit withdrawal amount
    account.info.balance = account.info.balance.checked_sub(amount_wei).unwrap();
    account.mark_touch();
    // Commit changes to database
    db.commit([(address, account)].into());

    Ok(())
}
