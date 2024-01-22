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

//! Convert from Ethers types.
// #[cfg(not(feature = "std"))]
// use crate::no_std_preflight::*;

extern crate alloc;
extern crate core;

pub use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
pub use core::{
    num::TryFromIntError,
    convert::From,
    default::Default,
    option::{Option, Option::*},
    result::{Result, Result::*},
};

use alloy_primitives::{Address, Bloom, Bytes, B256, U256};
use anyhow::{anyhow, Context};
use ethers_core::types::{
    transaction::eip2930::{
        AccessList as EthersAccessList, AccessListItem as EthersAccessListItem,
    },
    Block as EthersBlock, Bytes as EthersBytes, EIP1186ProofResponse,
    Transaction as EthersTransaction, TransactionReceipt as EthersReceipt,
    Withdrawal as EthersWithdrawal, H160 as EthersH160, H256 as EthersH256, U256 as EthersU256,
    U64,
};

use crate::{
    access_list::{AccessList, AccessListItem},
    block::Header,
    receipt::{Log, Receipt, ReceiptPayload},
    transactions::{
        ethereum::{
            EthereumTxEssence, TransactionKind, TxEssenceEip1559, TxEssenceEip2930, TxEssenceLegacy,
        },
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        signature::TxSignature,
        Transaction, TxEssence,
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

/// Conversion from `EthersAccessListItem` to the local [AccessListItem].
impl From<EthersAccessListItem> for AccessListItem {
    fn from(item: EthersAccessListItem) -> Self {
        AccessListItem {
            address: item.address.0.into(),
            storage_keys: item
                .storage_keys
                .into_iter()
                .map(|key| key.0.into())
                .collect(),
        }
    }
}

/// Conversion from `EthersAccessList` to the local [AccessList].
impl From<EthersAccessList> for AccessList {
    fn from(list: EthersAccessList) -> Self {
        AccessList(list.0.into_iter().map(|item| item.into()).collect())
    }
}

/// Convert an optional `EthersH160` to the local [TransactionKind].
impl From<Option<EthersH160>> for TransactionKind {
    fn from(addr: Option<EthersH160>) -> Self {
        match addr {
            Some(address) => TransactionKind::Call(address.0.into()),
            None => TransactionKind::Create,
        }
    }
}

/// Conversion from `EthersBlock` to the local [Header].
/// This conversion may fail if certain expected fields are missing.
impl<T> TryFrom<EthersBlock<T>> for Header {
    type Error = anyhow::Error;

    fn try_from(block: EthersBlock<T>) -> Result<Self, Self::Error> {
        Ok(Header {
            parent_hash: from_ethers_h256(block.parent_hash),
            ommers_hash: from_ethers_h256(block.uncles_hash),
            beneficiary: from_ethers_h160(block.author.context("author missing")?),
            state_root: from_ethers_h256(block.state_root),
            transactions_root: from_ethers_h256(block.transactions_root),
            receipts_root: from_ethers_h256(block.receipts_root),
            logs_bloom: Bloom::from_slice(
                block.logs_bloom.context("logs_bloom missing")?.as_bytes(),
            ),
            difficulty: from_ethers_u256(block.difficulty),
            number: block.number.context("number missing")?.as_u64(),
            gas_limit: from_ethers_u256(block.gas_limit),
            gas_used: from_ethers_u256(block.gas_used),
            timestamp: from_ethers_u256(block.timestamp),
            extra_data: block.extra_data.0.into(),
            mix_hash: block.mix_hash.context("mix_hash missing")?.0.into(),
            nonce: block.nonce.context("nonce missing")?.0.into(),
            base_fee_per_gas: from_ethers_u256(
                block.base_fee_per_gas.context("base_fee_per_gas missing")?,
            ),
            withdrawals_root: block.withdrawals_root.map(from_ethers_h256),
        })
    }
}

/// Conversion from `EthersTransaction` to the local [Transaction].
/// This conversion may fail if certain expected fields are missing.
impl<E: TxEssence + TryFrom<EthersTransaction>> TryFrom<EthersTransaction> for Transaction<E> {
    type Error = <E as TryFrom<EthersTransaction>>::Error;

    fn try_from(value: EthersTransaction) -> Result<Self, Self::Error> {
        let signature = TxSignature {
            v: value.v.as_u64(),
            r: from_ethers_u256(value.r),
            s: from_ethers_u256(value.s),
        };
        let essence = value.try_into()?;

        Ok(Transaction { essence, signature })
    }
}

/// Conversion from `EthersTransaction` to the local [EthereumTxEssence].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<EthersTransaction> for EthereumTxEssence {
    type Error = anyhow::Error;

    fn try_from(tx: EthersTransaction) -> Result<Self, Self::Error> {
        let essence = match tx.transaction_type.map(|t| t.as_u64()) {
            None | Some(0) => EthereumTxEssence::Legacy(TxEssenceLegacy {
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
                gas_price: from_ethers_u256(tx.gas_price.context("gas_price missing")?),
                gas_limit: from_ethers_u256(tx.gas),
                to: tx.to.into(),
                value: from_ethers_u256(tx.value),
                data: tx.input.0.into(),
            }),
            Some(1) => EthereumTxEssence::Eip2930(TxEssenceEip2930 {
                chain_id: tx
                    .chain_id
                    .context("chain_id missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid chain_id: {}", err))?,
                nonce: tx
                    .nonce
                    .try_into()
                    .map_err(|err| anyhow!("invalid nonce: {}", err))?,
                gas_price: from_ethers_u256(tx.gas_price.context("gas_price missing")?),
                gas_limit: from_ethers_u256(tx.gas),
                to: tx.to.into(),
                value: from_ethers_u256(tx.value),
                access_list: tx.access_list.context("access_list missing")?.into(),
                data: tx.input.0.into(),
            }),
            Some(2) => EthereumTxEssence::Eip1559(TxEssenceEip1559 {
                chain_id: tx
                    .chain_id
                    .context("chain_id missing")?
                    .try_into()
                    .map_err(|err| anyhow!("invalid chain_id: {}", err))?,
                nonce: tx
                    .nonce
                    .try_into()
                    .map_err(|err| anyhow!("invalid nonce: {}", err))?,
                max_priority_fee_per_gas: from_ethers_u256(
                    tx.max_priority_fee_per_gas
                        .context("max_priority_fee_per_gas missing")?,
                ),
                max_fee_per_gas: from_ethers_u256(
                    tx.max_fee_per_gas.context("max_fee_per_gas missing")?,
                ),
                gas_limit: from_ethers_u256(tx.gas),
                to: tx.to.into(),
                value: from_ethers_u256(tx.value),
                access_list: tx.access_list.context("access_list missing")?.into(),
                data: tx.input.0.into(),
            }),
            _ => unreachable!(),
        };
        Ok(essence)
    }
}

/// Conversion from `EthersTransaction` to the local [OptimismTxEssence].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<EthersTransaction> for OptimismTxEssence {
    type Error = anyhow::Error;

    fn try_from(tx: EthersTransaction) -> Result<Self, Self::Error> {
        let essence = match tx.transaction_type.map(|t| t.as_u64()) {
            Some(0x7E) => OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
                gas_limit: from_ethers_u256(tx.gas),
                from: tx.from.0.into(),
                to: tx.to.into(),
                value: from_ethers_u256(tx.value),
                data: tx.input.0.into(),
                source_hash: from_ethers_h256(tx.source_hash),
                mint: from_ethers_u256(tx.mint.context("mint missing")?),
                is_system_tx: tx.is_system_tx,
            }),
            _ => OptimismTxEssence::Ethereum(tx.try_into()?),
        };
        Ok(essence)
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

impl TryFrom<EthersReceipt> for Receipt {
    type Error = anyhow::Error;

    fn try_from(receipt: EthersReceipt) -> Result<Self, Self::Error> {
        Ok(Receipt {
            tx_type: receipt
                .transaction_type
                .context("transaction_type missing")?
                .as_u64()
                .try_into()
                .map_err(|e: TryFromIntError| anyhow!(e))
                .context("invalid transaction_type")?,
            payload: ReceiptPayload {
                success: receipt.status.context("status missing")? == U64::one(),
                cumulative_gas_used: from_ethers_u256(receipt.cumulative_gas_used),
                logs_bloom: Bloom::from_slice(receipt.logs_bloom.as_bytes()),
                logs: receipt
                    .logs
                    .into_iter()
                    .map(|log| {
                        let address = log.address.0.into();
                        let topics = log.topics.into_iter().map(from_ethers_h256).collect();
                        let data = log.data.0.into();
                        Log {
                            address,
                            topics,
                            data,
                        }
                    })
                    .collect(),
            },
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
