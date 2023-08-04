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

use anyhow::bail;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use zeth_lib::{
    block_builder::BlockBuilderDatabase,
    host::provider::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery},
    mem_db::DbAccount,
};
use zeth_primitives::{
    access_list::{AccessList, AccessListItem},
    block::Header,
    ethers::from_ethers_h160,
    keccak::keccak,
    revm::from_revm_b160,
    signature::TxSignature,
    transaction::{
        Transaction, TransactionKind, TxEssence, TxEssenceEip1559, TxEssenceEip2930,
        TxEssenceLegacy,
    },
    trie::{self, MptNode, MptNodeData, StateAccount},
    withdrawal::Withdrawal,
    Bloom, Bytes, RlpBytes, StorageKey, B160, B256, B64, U256, U64,
};

pub mod ethers;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestJson {
    pub blocks: Vec<TestBlock>,
    #[serde(rename = "genesisBlockHeader")]
    pub genesis: TestHeader,
    #[serde(rename = "genesisRLP")]
    pub genesis_rlp: Bytes,
    pub network: String,
    pub pre: TestState,
    #[serde(rename = "postState")]
    pub post: Option<TestState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBlock {
    pub block_header: Option<TestHeader>,
    pub expect_exception: Option<String>,
    pub rlp: Bytes,
    #[serde(default)]
    pub transactions: Vec<TestTransaction>,
    #[serde(default)]
    pub uncle_headers: Vec<TestHeader>,
    pub withdrawals: Option<Vec<Withdrawal>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestAccount {
    pub balance: U256,
    pub nonce: U64,
    pub code: Bytes,
    pub storage: HashMap<U256, U256>,
}

impl From<DbAccount> for TestAccount {
    fn from(account: DbAccount) -> Self {
        TestAccount {
            balance: account.info.balance,
            nonce: U64::from(account.info.nonce),
            code: account.info.code.unwrap().bytecode.into(),
            storage: account.storage.into_iter().map(|(k, v)| (k, v)).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestState(pub HashMap<B160, TestAccount>);

impl<D: BlockBuilderDatabase> From<D> for TestState {
    fn from(db: D) -> Self {
        TestState(
            db.accounts()
                .map(|(addr, account)| (from_revm_b160(*addr), account.clone().into()))
                .collect(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestHeader {
    pub base_fee_per_gas: Option<U256>,
    pub bloom: Bloom,
    pub coinbase: B160,
    pub extra_data: Bytes,
    pub difficulty: U256,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub hash: B256,
    pub mix_hash: B256,
    pub nonce: B64,
    pub number: U64,
    pub parent_hash: B256,
    pub receipt_trie: B256,
    pub state_root: B256,
    pub timestamp: U256,
    pub transactions_trie: B256,
    pub uncle_hash: B256,
    pub withdrawals_root: Option<B256>,
}

impl From<TestHeader> for Header {
    fn from(header: TestHeader) -> Self {
        Header {
            parent_hash: header.parent_hash,
            ommers_hash: header.uncle_hash,
            beneficiary: header.coinbase,
            state_root: header.state_root,
            transactions_root: header.transactions_trie,
            receipts_root: header.receipt_trie,
            logs_bloom: header.bloom,
            difficulty: header.difficulty,
            number: header.number.try_into().unwrap(),
            gas_limit: header.gas_limit,
            gas_used: header.gas_used,
            timestamp: header.timestamp,
            extra_data: header.extra_data,
            mix_hash: header.mix_hash,
            nonce: header.nonce,
            base_fee_per_gas: header.base_fee_per_gas.unwrap(),
            withdrawals_root: header.withdrawals_root,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTransaction {
    pub data: Bytes,
    pub access_list: Option<TestAccessList>,
    pub gas_limit: U256,
    pub gas_price: Option<U256>,
    pub max_fee_per_gas: Option<U256>,
    pub max_priority_fee_per_gas: Option<U256>,
    pub value: U256,
    #[serde_as(as = "NoneAsEmptyString")]
    pub to: Option<B160>,
    pub nonce: U64,
    pub v: U64,
    pub r: U256,
    pub s: U256,
}

impl From<TestTransaction> for Transaction {
    fn from(tx: TestTransaction) -> Self {
        let signature = TxSignature {
            v: tx.v.try_into().unwrap(),
            r: tx.r,
            s: tx.s,
        };
        let essence = if tx.access_list.is_none() {
            TxEssence::Legacy(TxEssenceLegacy {
                chain_id: None,
                nonce: tx.nonce.try_into().unwrap(),
                gas_price: tx.gas_price.unwrap(),
                gas_limit: tx.gas_limit,
                to: match tx.to {
                    Some(addr) => TransactionKind::Call(addr),
                    None => TransactionKind::Create,
                },
                value: tx.value,
                data: tx.data,
            })
        } else if tx.max_fee_per_gas.is_none() {
            TxEssence::Eip2930(TxEssenceEip2930 {
                chain_id: 1,
                nonce: tx.nonce.try_into().unwrap(),
                gas_price: tx.gas_price.unwrap(),
                gas_limit: tx.gas_limit,
                to: match tx.to {
                    Some(addr) => TransactionKind::Call(addr),
                    None => TransactionKind::Create,
                },
                value: tx.value,
                data: tx.data,
                access_list: tx.access_list.unwrap().into(),
            })
        } else {
            TxEssence::Eip1559(TxEssenceEip1559 {
                chain_id: 1,
                nonce: tx.nonce.try_into().unwrap(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.unwrap(),
                max_fee_per_gas: tx.max_fee_per_gas.unwrap(),
                gas_limit: tx.gas_limit,
                to: match tx.to {
                    Some(addr) => TransactionKind::Call(addr),
                    None => TransactionKind::Create,
                },
                value: tx.value,
                data: tx.data,
                access_list: tx.access_list.unwrap().into(),
            })
        };
        Transaction { essence, signature }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccessList(pub Vec<TestAccessListItem>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccessListItem {
    pub address: B160,
    pub storage_keys: Vec<StorageKey>,
}

impl From<TestAccessList> for AccessList {
    fn from(list: TestAccessList) -> Self {
        AccessList(
            list.0
                .into_iter()
                .map(|item| AccessListItem {
                    address: item.address,
                    storage_keys: item.storage_keys,
                })
                .collect(),
        )
    }
}

/// Computes the Merkle proof for the given key in the trie.
pub fn mpt_proof(root: &MptNode, key: impl AsRef<[u8]>) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mut path = proof_internal(root, &trie::to_nibs(key.as_ref()))?;
    path.reverse();
    Ok(path)
}

fn proof_internal(node: &MptNode, key_nibs: &[u8]) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    if key_nibs.is_empty() {
        return Ok(vec![node.to_rlp()]);
    }

    let mut path: Vec<Vec<u8>> = match node.as_data() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => vec![],
        MptNodeData::Branch(children, _) => {
            let mut path = Vec::new();
            for node in children {
                path.extend(proof_internal(&node, &key_nibs[1..])?);
            }
            path
        }
        MptNodeData::Extension(_, child) => {
            let ext_nibs = node.nibs();
            let ext_len = ext_nibs.len();
            if key_nibs[..ext_len] == ext_nibs {
                proof_internal(&child, &key_nibs[ext_len..])?
            } else {
                vec![]
            }
        }
        MptNodeData::Digest(_) => bail!("Cannot descend pointer!"),
    };
    path.push(node.to_rlp());

    Ok(path)
}
