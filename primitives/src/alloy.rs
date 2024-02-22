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

//! Convert from Alloy types.

use crate::{
    access_list::{AccessList, AccessListItem},
    block::Header,
    receipt::{Log, Receipt, ReceiptPayload, OPTIMISM_DEPOSIT_NONCE_VERSION},
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
use alloy::rpc::types::eth::{
    AccessList as AlloyAccessList, AccessListItem as AlloyAccessListItem,
    EIP1186AccountProofResponse, Header as AlloyHeader, Transaction as AlloyTransaction,
    TransactionReceipt as AlloyReceipt, Withdrawal as AlloyWithdrawal,
};

use alloy_primitives::{Address, B256, U256, U64};
use anyhow::{anyhow, Context};
use serde::Deserialize;

/// Conversion from `AlloyAccessListItem` to the local [AccessListItem].
impl From<AlloyAccessListItem> for AccessListItem {
    fn from(item: AlloyAccessListItem) -> Self {
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

/// Conversion from `AlloyAccessList` to the local [AccessList].
impl From<AlloyAccessList> for AccessList {
    fn from(list: AlloyAccessList) -> Self {
        AccessList(list.0.into_iter().map(|item| item.into()).collect())
    }
}

/// Convert an optional `Address` to the local [TransactionKind].
impl From<Option<Address>> for TransactionKind {
    fn from(addr: Option<Address>) -> Self {
        match addr {
            Some(address) => TransactionKind::Call(address),
            None => TransactionKind::Create,
        }
    }
}

/// Conversion from `AlloyHeader` to the local [Header].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<AlloyHeader> for Header {
    type Error = anyhow::Error;

    fn try_from(header: AlloyHeader) -> Result<Self, Self::Error> {
        Ok(Header {
            parent_hash: header.parent_hash,
            ommers_hash: header.uncles_hash,
            beneficiary: header.miner,
            state_root: header.state_root,
            transactions_root: header.transactions_root,
            receipts_root: header.receipts_root,
            logs_bloom: header.logs_bloom,
            difficulty: header.difficulty,
            number: header
                .number
                .context("number missing")?
                .try_into()
                .context("invalid number")?,
            gas_limit: header.gas_limit,
            gas_used: header.gas_used,
            timestamp: header.timestamp,
            extra_data: header.extra_data,
            mix_hash: header.mix_hash.context("mix_hash missing")?,
            nonce: header.nonce.context("nonce missing")?,
            base_fee_per_gas: header
                .base_fee_per_gas
                .context("base_fee_per_gas missing")?,
            withdrawals_root: header.withdrawals_root,
        })
    }
}

/// Conversion from `AlloyTransaction` to the local [Transaction].
/// This conversion may fail if certain expected fields are missing.
impl<E: TxEssence + TryFrom<AlloyTransaction, Error = anyhow::Error>> TryFrom<AlloyTransaction>
    for Transaction<E>
{
    type Error = anyhow::Error;

    fn try_from(value: AlloyTransaction) -> Result<Self, Self::Error> {
        let alloy_signature = value.signature.context("signature missing")?;
        let signature = TxSignature {
            v: alloy_signature.v.try_into().context("invalid v")?,
            r: alloy_signature.r,
            s: alloy_signature.s,
        };
        let essence = value.try_into()?;

        Ok(Transaction { essence, signature })
    }
}

/// Conversion from `AlloyTransaction` to the local [EthereumTxEssence].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<AlloyTransaction> for EthereumTxEssence {
    type Error = anyhow::Error;

    fn try_from(tx: AlloyTransaction) -> Result<Self, Self::Error> {
        let transaction_type: Option<u64> = tx.transaction_type.and_then(|t| t.try_into().ok());
        let essence = match transaction_type {
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
                gas_price: U256::from_le_slice(
                    tx.gas_price.context("gas_price missing")?.as_le_slice(),
                ),
                gas_limit: tx.gas,
                to: tx.to.into(),
                value: tx.value,
                data: tx.input,
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
                gas_price: U256::from_le_slice(
                    tx.gas_price.context("gas_price missing")?.as_le_slice(),
                ),
                gas_limit: tx.gas,
                to: tx.to.into(),
                value: tx.value,
                access_list: AccessList(
                    tx.access_list
                        .context("access_list missing")?
                        .into_iter()
                        .map(|v| v.into())
                        .collect(),
                ),
                data: tx.input,
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
                max_priority_fee_per_gas: U256::from_le_slice(
                    tx.max_priority_fee_per_gas
                        .context("max_priority_fee_per_gas missing")?
                        .as_le_slice(),
                ),
                max_fee_per_gas: U256::from_le_slice(
                    tx.max_fee_per_gas
                        .context("max_fee_per_gas missing")?
                        .as_le_slice(),
                ),
                gas_limit: tx.gas,
                to: tx.to.into(),
                value: tx.value,
                access_list: AccessList(
                    tx.access_list
                        .context("access_list missing")?
                        .into_iter()
                        .map(|v| v.into())
                        .collect(),
                ),
                data: tx.input,
            }),
            _ => unreachable!(),
        };
        Ok(essence)
    }
}

/// Conversion from `AlloyTransaction` to the local [OptimismTxEssence].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<AlloyTransaction> for OptimismTxEssence {
    type Error = anyhow::Error;

    fn try_from(tx: AlloyTransaction) -> Result<Self, Self::Error> {
        let transaction_type: Option<u64> = tx.transaction_type.and_then(|t| t.try_into().ok());
        let source_hash =
            B256::deserialize(tx.other.get("sourceHash").context("sourceHash missing")?)?;
        let mint = U256::deserialize(tx.other.get("mint").context("mint missing")?)?;
        let is_system_tx = tx
            .other
            .get("isSystemTx")
            .and_then(|v| v.as_bool())
            .context("isSystemTx missing")?;
        let essence = match transaction_type {
            Some(0x7E) => OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
                gas_limit: tx.gas,
                from: tx.from,
                to: tx.to.into(),
                value: tx.value,
                data: tx.input,
                source_hash,
                mint: mint,
                is_system_tx,
            }),
            _ => OptimismTxEssence::Ethereum(tx.try_into()?),
        };
        Ok(essence)
    }
}

/// Conversion from `AlloyWithdrawal` to the local [Withdrawal].
/// This conversion may fail if certain expected fields are missing.
impl TryFrom<AlloyWithdrawal> for Withdrawal {
    type Error = anyhow::Error;

    fn try_from(withdrawal: AlloyWithdrawal) -> Result<Self, Self::Error> {
        Ok(Withdrawal {
            index: withdrawal.index.try_into().context("invalid index")?,
            validator_index: withdrawal
                .validator_index
                .try_into()
                .context("invalid validator index")?,
            address: withdrawal.address.0.into(),
            amount: withdrawal
                .amount
                .try_into()
                .map_err(|err| anyhow!("invalid amount: {}", err))?,
        })
    }
}

impl TryFrom<AlloyReceipt> for Receipt {
    type Error = anyhow::Error;

    fn try_from(receipt: AlloyReceipt) -> Result<Self, Self::Error> {
        let deposit_nonce = receipt.other.get("depositNonce").and_then(|v| v.as_u64());
        Ok(Receipt {
            tx_type: receipt
                .transaction_type
                .try_into()
                .map_err(|err| anyhow!("invalid transaction type: {}", err))?,
            payload: ReceiptPayload {
                success: receipt.status_code.context("status missing")? == U64::from(1),
                cumulative_gas_used: receipt.cumulative_gas_used,
                logs_bloom: receipt.logs_bloom,
                logs: receipt
                    .logs
                    .into_iter()
                    .map(|log| {
                        let address = log.address.0.into();
                        let topics = log.topics.into_iter().collect();
                        let data = log.data.0.into();
                        Log {
                            address,
                            topics,
                            data,
                        }
                    })
                    .collect(),
                deposit_nonce: deposit_nonce,
                deposit_nonce_version: deposit_nonce.map(|_| OPTIMISM_DEPOSIT_NONCE_VERSION),
            },
        })
    }
}

/// Conversion from `EIP1186ProofResponse` to the local [StateAccount].
impl From<EIP1186AccountProofResponse> for StateAccount {
    fn from(response: EIP1186AccountProofResponse) -> Self {
        StateAccount {
            nonce: response.nonce.try_into().unwrap_or_default(),
            balance: response.balance,
            storage_root: response.storage_hash,
            code_hash: response.code_hash,
        }
    }
}
