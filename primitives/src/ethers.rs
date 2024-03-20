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

//! Convert from Ethers types.

use alloy_consensus::{Header, ReceiptEnvelope as AlloyReceiptEnvelope, SignableTransaction};
use alloy_primitives::{Address, Bloom, Bytes, Log, Signature, B256, U256};
use anyhow::{anyhow, Context};
use ethers_core::types::{
    transaction::eip2930::AccessList as EthersAccessList, Block as EthersBlock,
    Bytes as EthersBytes, EIP1186ProofResponse, Transaction as EthersTransaction,
    TransactionReceipt as EthersReceipt, Withdrawal as EthersWithdrawal, H160 as EthersH160,
    H256 as EthersH256, U256 as EthersU256, U64,
};

use crate::{
    access_list::{AccessList, AccessListItem},
    receipt::{OptimismDepositReceipt, Receipt, ReceiptEnvelope, ReceiptWithBloom},
    transactions::{
        optimism::TxOptimismDeposit, TxEip1559, TxEip2930, TxEnvelope, TxLegacy, TxType,
    },
    trie::StateAccount,
    withdrawal::Withdrawal,
};

/// Convert an `EthersU256` type to the `U256` type.
#[inline]
pub fn from_ethers_u256(v: EthersU256) -> U256 {
    U256::from_limbs(v.0)
}

/// Convert an `U256` type to the `EthersU256` type.
#[inline]
pub fn to_ethers_u256(v: U256) -> EthersU256 {
    EthersU256(v.into_limbs())
}

/// Convert an `EthersH160` type to the `Address` type.
#[inline]
pub fn from_ethers_h160(v: EthersH160) -> Address {
    v.0.into()
}

/// Convert an `EthersH256` type to the `B256` type.
#[inline]
pub fn from_ethers_h256(v: EthersH256) -> B256 {
    v.0.into()
}

/// Convert an `EthersBytes` type to the `Bytes` type.
#[inline]
pub fn from_ethers_bytes(v: EthersBytes) -> Bytes {
    v.0.into()
}

/// Convert an `AccessList` type to the `EthersAccessList` type.
pub fn from_ethers_access_list(list: EthersAccessList) -> AccessList {
    let items = list
        .0
        .into_iter()
        .map(|item| AccessListItem {
            address: from_ethers_h160(item.address),
            storage_keys: item
                .storage_keys
                .into_iter()
                .map(from_ethers_h256)
                .collect(),
        })
        .collect();
    AccessList(items)
}

fn from_ethers_transaction_type(v: Option<U64>) -> Result<Option<TxType>, anyhow::Error> {
    let tx_type = match v {
        Some(v) => {
            let v: u8 = v
                .try_into()
                .map_err(|err| anyhow!("invalid transaction_type: {}", err))?;
            Some(TxType::try_from(v).context("invalid transaction_type")?)
        }
        None => None,
    };
    Ok(tx_type)
}

/// Conversion from `EthersBlock` to the local [Header].
/// This conversion may fail if certain expected fields are missing.
pub fn from_ethers_block<T>(block: EthersBlock<T>) -> Result<Header, anyhow::Error> {
    Ok(Header {
        parent_hash: from_ethers_h256(block.parent_hash),
        ommers_hash: from_ethers_h256(block.uncles_hash),
        beneficiary: from_ethers_h160(block.author.context("author missing")?),
        state_root: from_ethers_h256(block.state_root),
        transactions_root: from_ethers_h256(block.transactions_root),
        receipts_root: from_ethers_h256(block.receipts_root),
        withdrawals_root: block.withdrawals_root.map(from_ethers_h256),
        logs_bloom: Bloom::from_slice(block.logs_bloom.context("logs_bloom missing")?.as_bytes()),
        difficulty: from_ethers_u256(block.difficulty),
        number: block.number.context("number missing")?.as_u64(),
        gas_limit: block
            .gas_limit
            .try_into()
            .map_err(|err| anyhow!("invalid gas_limit: {}", err))?,
        gas_used: block
            .gas_used
            .try_into()
            .map_err(|err| anyhow!("invalid gas_used: {}", err))?,
        timestamp: block
            .timestamp
            .try_into()
            .map_err(|err| anyhow!("invalid timestamp: {}", err))?,
        mix_hash: block
            .mix_hash
            .map(from_ethers_h256)
            .context("mix_hash missing")?,
        nonce: block.nonce.context("nonce missing")?.to_low_u64_be(),
        base_fee_per_gas: block
            .base_fee_per_gas
            .map(u64::try_from)
            .transpose()
            .map_err(|err| anyhow!("invalid base_fee_per_gas: {}", err))?,
        blob_gas_used: block
            .blob_gas_used
            .map(u64::try_from)
            .transpose()
            .map_err(|err| anyhow!("invalid blob_gas_used: {}", err))?,
        excess_blob_gas: block
            .excess_blob_gas
            .map(u64::try_from)
            .transpose()
            .map_err(|err| anyhow!("invalid excess_blob_gas: {}", err))?,
        parent_beacon_block_root: block.parent_beacon_block_root.map(from_ethers_h256),
        extra_data: from_ethers_bytes(block.extra_data),
    })
}

impl TryFrom<EthersTransaction> for TxEnvelope {
    type Error = anyhow::Error;

    fn try_from(tx: EthersTransaction) -> Result<Self, Self::Error> {
        let tx_type = from_ethers_transaction_type(tx.transaction_type)?;

        let signature = if tx_type != Some(TxType::OptimismDeposit) {
            Some(Signature::from_rs_and_parity(
                from_ethers_u256(tx.r),
                from_ethers_u256(tx.s),
                tx.v.as_u64(),
            )?)
        } else {
            None
        };

        let tx = match tx_type {
            None | Some(TxType::Legacy) => TxLegacy {
                chain_id: match tx.chain_id {
                    None => None,
                    Some(chain_id) => Some(
                        chain_id
                            .try_into()
                            .map_err(|err| anyhow!("invalid chain_id: {}", err))?,
                    ),
                },
                nonce: tx
                    .nonce
                    .try_into()
                    .map_err(|err| anyhow!("invalid nonce: {}", err))?,
                gas_price: tx
                    .gas_price
                    .context("gas_price missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_price: {}", err))?,
                gas_limit: tx
                    .gas
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_limit: {}", err))?,
                to: tx.to.map(from_ethers_h160).into(),
                value: from_ethers_u256(tx.value),
                input: tx.input.0.into(),
            }
            .into_signed(signature.unwrap())
            .into(),
            Some(TxType::Eip2930) => TxEip2930 {
                chain_id: tx
                    .chain_id
                    .context("chain_id missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid chain_id: {}", err))?,
                nonce: tx
                    .nonce
                    .try_into()
                    .map_err(|err| anyhow!("invalid nonce: {}", err))?,
                gas_price: tx
                    .gas_price
                    .context("gas_price missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_price: {}", err))?,
                gas_limit: tx
                    .gas
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_limit: {}", err))?,
                to: tx.to.map(from_ethers_h160).into(),
                value: from_ethers_u256(tx.value),
                access_list: from_ethers_access_list(
                    tx.access_list.context("access_list missing")?,
                ),
                input: tx.input.0.into(),
            }
            .into_signed(signature.unwrap())
            .into(),
            Some(TxType::Eip1559) => TxEip1559 {
                chain_id: tx
                    .chain_id
                    .context("chain_id missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid chain_id: {}", err))?,
                nonce: tx
                    .nonce
                    .try_into()
                    .map_err(|err| anyhow!("invalid nonce: {}", err))?,
                max_priority_fee_per_gas: tx
                    .max_priority_fee_per_gas
                    .context("max_priority_fee_per_gas missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid max_priority_fee_per_gas: {}", err))?,
                max_fee_per_gas: tx
                    .max_fee_per_gas
                    .context("max_fee_per_gas missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid max_fee_per_gas: {}", err))?,
                gas_limit: tx
                    .gas
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_limit: {}", err))?,
                to: tx.to.map(from_ethers_h160).into(),
                value: from_ethers_u256(tx.value),
                access_list: from_ethers_access_list(
                    tx.access_list.context("access_list missing")?,
                ),
                input: tx.input.0.into(),
            }
            .into_signed(signature.unwrap())
            .into(),
            Some(TxType::Eip4844) => unimplemented!("EIP-4844 not supported"),
            Some(TxType::OptimismDeposit) => TxEnvelope::OptimismDeposit(TxOptimismDeposit {
                source_hash: from_ethers_h256(tx.source_hash),
                from: tx.from.0.into(),
                to: tx.to.map(from_ethers_h160).into(),
                mint: from_ethers_u256(tx.mint.context("mint missing")?),
                value: from_ethers_u256(tx.value),
                gas_limit: tx
                    .gas
                    .try_into()
                    .map_err(|err| anyhow!("invalid gas_limit: {}", err))?,
                is_system_tx: tx.is_system_tx,
                input: tx.input.0.into(),
            }),
        };

        Ok(tx)
    }
}

/// Conversion from `EthersWithdrawal` to the local [Withdrawal].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<EthersWithdrawal> for Withdrawal {
    type Error = anyhow::Error;

    fn try_from(withdrawal: EthersWithdrawal) -> Result<Self, Self::Error> {
        Ok(Withdrawal {
            index: withdrawal.index.as_u64(),
            validator_index: withdrawal.validator_index.as_u64(),
            address: withdrawal.address.0.into(),
            amount: withdrawal
                .amount
                .try_into()
                .map_err(|err| anyhow!("invalid amount: {}", err))?,
        })
    }
}

impl TryFrom<EthersReceipt> for ReceiptEnvelope {
    type Error = anyhow::Error;

    fn try_from(v: EthersReceipt) -> Result<Self, Self::Error> {
        let tx_type = from_ethers_transaction_type(v.transaction_type)
            .map_err(|err| anyhow!("invalid transaction_type: {}", err))?;

        let receipt = Receipt {
            success: v.status.context("status missing")? == U64::one(),
            cumulative_gas_used: v.cumulative_gas_used.try_into().unwrap(),
            logs: v
                .logs
                .into_iter()
                .map(|log| {
                    Log::new_unchecked(
                        from_ethers_h160(log.address),
                        log.topics.into_iter().map(from_ethers_h256).collect(),
                        from_ethers_bytes(log.data),
                    )
                })
                .collect(),
        };
        let receipt = ReceiptWithBloom::new(receipt, Bloom::from_slice(v.logs_bloom.as_bytes()));

        Ok(match tx_type {
            None | Some(TxType::Legacy) => {
                ReceiptEnvelope::Ethereum(AlloyReceiptEnvelope::Legacy(receipt))
            }
            Some(TxType::Eip2930) => {
                ReceiptEnvelope::Ethereum(AlloyReceiptEnvelope::Eip2930(receipt))
            }
            Some(TxType::Eip1559) => {
                ReceiptEnvelope::Ethereum(AlloyReceiptEnvelope::Eip1559(receipt))
            }
            Some(TxType::Eip4844) => {
                ReceiptEnvelope::Ethereum(AlloyReceiptEnvelope::Eip4844(receipt))
            }
            Some(TxType::OptimismDeposit) => {
                let receipt = OptimismDepositReceipt::new(receipt, v.deposit_nonce);
                ReceiptEnvelope::OptimismDeposit(receipt)
            }
        })
    }
}

/// Conversion from `EIP1186ProofResponse` to the local [StateAccount].
impl From<EIP1186ProofResponse> for StateAccount {
    fn from(response: EIP1186ProofResponse) -> Self {
        StateAccount {
            nonce: response.nonce.as_u64(),
            balance: from_ethers_u256(response.balance),
            storage_root: from_ethers_h256(response.storage_hash),
            code_hash: from_ethers_h256(response.code_hash),
        }
    }
}
