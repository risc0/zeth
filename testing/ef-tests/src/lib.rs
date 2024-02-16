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

#![cfg(feature = "ef-tests")]

use anyhow::bail;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use zeth_lib::{
    builder::{BlockBuilder, BlockBuilderStrategy, EthereumStrategy},
    consts::ChainSpec,
    host::{
        preflight::Data,
        provider::{AccountQuery, BlockQuery, ProofQuery, Provider, StorageQuery},
        provider_db::ProviderDb,
    },
    input::{BlockBuildInput, StateInput},
    mem_db::{AccountState, DbAccount, MemDb},
};
use zeth_primitives::{
    access_list::{AccessList, AccessListItem},
    alloy_rlp,
    block::Header,
    ethers::from_ethers_h160,
    keccak::keccak,
    transactions::{SignableTransaction, TxEip1559, TxEip2930, TxEnvelope, TxLegacy},
    trie::{self, MptNode, MptNodeData, StateAccount},
    withdrawal::Withdrawal,
    Address, Bloom, Bytes, Signature, StorageKey, B256, B64, U128, U256, U64,
};

use crate::ethers::TestProvider;

pub mod ethers;
pub mod ethtests;

pub mod guests {
    include!(concat!(env!("OUT_DIR"), "/methods.rs"));
}

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
            code: account.info.code.unwrap().bytecode,
            storage: account.storage.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestState(pub HashMap<Address, TestAccount>);

impl From<&MemDb> for TestState {
    fn from(db: &MemDb) -> Self {
        TestState(
            db.accounts
                .iter()
                .filter(|(_, account)| account.state != AccountState::Deleted)
                .map(|(addr, account)| (*addr, account.clone().into()))
                .collect(),
        )
    }
}

impl From<&ProviderDb> for TestState {
    fn from(db: &ProviderDb) -> Self {
        (&db.latest_db).into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestHeader {
    pub base_fee_per_gas: Option<U64>,
    pub bloom: Bloom,
    pub coinbase: Address,
    pub extra_data: Bytes,
    pub difficulty: U256,
    pub gas_limit: U64,
    pub gas_used: U64,
    pub hash: B256,
    pub mix_hash: B256,
    pub nonce: B64,
    pub number: U64,
    pub parent_hash: B256,
    pub receipt_trie: B256,
    pub state_root: B256,
    pub timestamp: U64,
    pub transactions_trie: B256,
    pub uncle_hash: B256,
    pub withdrawals_root: Option<B256>,
    pub blob_gas_used: Option<U64>,
    pub excess_blob_gas: Option<U64>,
    pub parent_beacon_block_root: Option<B256>,
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
            gas_limit: header.gas_limit.try_into().unwrap(),
            gas_used: header.gas_used.try_into().unwrap(),
            timestamp: header.timestamp.try_into().unwrap(),
            extra_data: header.extra_data,
            mix_hash: header.mix_hash,
            nonce: header.nonce.into(),
            base_fee_per_gas: header.base_fee_per_gas.map(|v| v.try_into().unwrap()),
            withdrawals_root: header.withdrawals_root,
            blob_gas_used: header.blob_gas_used.map(|v| v.try_into().unwrap()),
            excess_blob_gas: header.excess_blob_gas.map(|v| v.try_into().unwrap()),
            parent_beacon_block_root: header.parent_beacon_block_root,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTransaction {
    pub data: Bytes,
    pub access_list: Option<TestAccessList>,
    pub gas_limit: U64,
    pub gas_price: Option<U128>,
    pub max_fee_per_gas: Option<U128>,
    pub max_priority_fee_per_gas: Option<U128>,
    pub value: U256,
    #[serde_as(as = "NoneAsEmptyString")]
    pub to: Option<Address>,
    pub nonce: U64,
    pub blob_versioned_hashes: Option<B256>,
    pub max_fee_per_blob_gas: Option<U128>,
    pub v: U64,
    pub r: U256,
    pub s: U256,
}

impl From<TestTransaction> for TxEnvelope {
    fn from(tx: TestTransaction) -> Self {
        let signature = Signature::from_rs_and_parity(tx.r, tx.s, tx.v).unwrap();

        if tx.access_list.is_none() {
            TxEnvelope::Legacy(
                TxLegacy {
                    chain_id: None,
                    nonce: tx.nonce.try_into().unwrap(),
                    gas_price: tx.gas_price.unwrap().try_into().unwrap(),
                    gas_limit: tx.gas_limit.try_into().unwrap(),
                    to: tx.to.into(),
                    value: tx.value,
                    input: tx.data,
                }
                .into_signed(signature),
            )
        } else if tx.max_fee_per_gas.is_none() {
            TxEnvelope::Eip2930(
                TxEip2930 {
                    chain_id: 1,
                    nonce: tx.nonce.try_into().unwrap(),
                    gas_price: tx.gas_price.unwrap().try_into().unwrap(),
                    gas_limit: tx.gas_limit.try_into().unwrap(),
                    to: tx.to.into(),
                    value: tx.value,
                    input: tx.data,
                    access_list: tx.access_list.unwrap().into(),
                }
                .into_signed(signature),
            )
        } else if tx.max_fee_per_blob_gas.is_none() {
            TxEnvelope::Eip1559(
                TxEip1559 {
                    chain_id: 1,
                    nonce: tx.nonce.try_into().unwrap(),
                    gas_limit: tx.gas_limit.try_into().unwrap(),
                    max_fee_per_gas: tx.max_fee_per_gas.unwrap().try_into().unwrap(),
                    max_priority_fee_per_gas: tx
                        .max_priority_fee_per_gas
                        .unwrap()
                        .try_into()
                        .unwrap(),
                    to: tx.to.into(),
                    value: tx.value,
                    input: tx.data,
                    access_list: tx.access_list.unwrap().into(),
                }
                .into_signed(signature),
            )
        } else {
            todo!("Eip4844 not implemented")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccessList(pub Vec<TestAccessListItem>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccessListItem {
    pub address: Address,
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
        return Ok(vec![alloy_rlp::encode(node)]);
    }

    let mut path: Vec<Vec<u8>> = match node.as_data() {
        MptNodeData::Null | MptNodeData::Leaf(_, _) => vec![],
        MptNodeData::Branch(children) => {
            let (i, tail) = key_nibs.split_first().unwrap();
            match &children[*i as usize] {
                Some(child) => proof_internal(child, tail)?,
                None => vec![],
            }
        }
        MptNodeData::Extension(_, child) => {
            if let Some(tail) = key_nibs.strip_prefix(node.nibs().as_slice()) {
                proof_internal(child, tail)?
            } else {
                vec![]
            }
        }
        MptNodeData::Digest(_) => bail!("Cannot descend pointer!"),
    };
    path.push(alloy_rlp::encode(node));

    Ok(path)
}

/// The size of the stack to use for the EVM.
pub const BIG_STACK_SIZE: usize = 8 * 1024 * 1024;

pub fn create_input(
    chain_spec: &ChainSpec,
    parent_header: Header,
    parent_state: TestState,
    header: Header,
    transactions: Vec<TestTransaction>,
    withdrawals: Vec<Withdrawal>,
    state: TestState,
) -> BlockBuildInput {
    // create the provider DB
    let provider_db = ProviderDb::new(
        Box::new(TestProvider {
            state: parent_state,
            header: parent_header.clone(),
            post: state,
        }),
        parent_header.number,
    );

    let transactions: Vec<TxEnvelope> = transactions.into_iter().map(TxEnvelope::from).collect();
    let input = BlockBuildInput {
        state_input: StateInput {
            parent_header: parent_header.clone(),
            beneficiary: header.beneficiary,
            gas_limit: header.gas_limit,
            timestamp: header.timestamp,
            extra_data: header.extra_data.clone(),
            mix_hash: header.mix_hash,
            transactions: transactions.clone(),
            withdrawals: withdrawals.clone(),
            parent_beacon_block_root: header.parent_beacon_block_root,
        },
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: vec![],

        ancestor_headers: vec![],
    };

    // create and run the block builder once to create the initial DB
    let mut builder = BlockBuilder::new(chain_spec, input, None)
        .with_db(provider_db)
        .prepare_header::<<EthereumStrategy as BlockBuilderStrategy>::HeaderPrepStrategy>()
        .unwrap()
        .execute_transactions::<<EthereumStrategy as BlockBuilderStrategy>::TxExecStrategy>()
        .unwrap();
    let provider_db = builder.mut_db().unwrap();

    let parent_proofs = provider_db.get_initial_proofs().unwrap();
    let proofs = provider_db.get_latest_proofs().unwrap();
    let ancestor_headers = provider_db.get_ancestor_headers().unwrap();

    let preflight_data = Data {
        db: provider_db.get_initial_db().clone(),
        parent_header,
        parent_proofs,
        header: Some(header),
        transactions,
        withdrawals,
        proofs,
        ancestor_headers,
    };

    preflight_data.try_into().unwrap()
}
