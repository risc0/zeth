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
use std::{collections::BTreeSet, path::PathBuf};

use alloy_primitives::{Address, TxHash};
use alloy_sol_types::{sol_data::Uint, SolEvent};
use anyhow::{anyhow, Context, Result};
use ethers_core::types::{
    Block, Bytes, EIP1186ProofResponse, Log, Transaction, TransactionReceipt, H160, H256, U256,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "taiko")]
use crate::taiko::BlockProposed;

pub mod cached_rpc_provider;
pub mod file_provider;
pub mod rpc_provider;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct AccountQuery {
    pub block_no: u64,
    pub address: H160,
}

#[derive(Clone, Debug, Deserialize, PartialOrd, Ord, Eq, Hash, PartialEq, Serialize)]
pub struct BlockQuery {
    pub block_no: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ProofQuery {
    pub block_no: u64,
    pub address: H160,
    pub indices: BTreeSet<H256>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct StorageQuery {
    pub block_no: u64,
    pub address: H160,
    pub index: H256,
}

#[cfg(feature = "taiko")]
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct LogsQuery {
    pub address: H160,
    pub from_block: u64,
    pub to_block: u64,
}

#[cfg(feature = "taiko")]
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TxQuery {
    pub tx_hash: H256,
    pub block_no: Option<u64>,
}

pub trait Provider: Send {
    fn save(&self) -> Result<()>;

    fn get_full_block(&mut self, query: &BlockQuery) -> Result<Block<Transaction>>;
    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>>;
    fn get_block_receipts(&mut self, query: &BlockQuery) -> Result<Vec<TransactionReceipt>>;
    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse>;
    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256>;
    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256>;
    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes>;
    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256>;
    #[cfg(feature = "taiko")]
    fn get_logs(&mut self, query: &LogsQuery) -> Result<Vec<Log>>;
    #[cfg(feature = "taiko")]
    fn get_transaction(&mut self, query: &TxQuery) -> Result<Transaction>;
}

pub trait MutProvider: Provider {
    fn insert_full_block(&mut self, query: BlockQuery, val: Block<Transaction>);
    fn insert_partial_block(&mut self, query: BlockQuery, val: Block<H256>);
    fn insert_block_receipts(&mut self, query: BlockQuery, val: Vec<TransactionReceipt>);
    fn insert_proof(&mut self, query: ProofQuery, val: EIP1186ProofResponse);
    fn insert_transaction_count(&mut self, query: AccountQuery, val: U256);
    fn insert_balance(&mut self, query: AccountQuery, val: U256);
    fn insert_code(&mut self, query: AccountQuery, val: Bytes);
    fn insert_storage(&mut self, query: StorageQuery, val: H256);
    #[cfg(feature = "taiko")]
    fn insert_logs(&mut self, query: LogsQuery, val: Vec<Log>);
    #[cfg(feature = "taiko")]
    fn insert_transaction(&mut self, query: TxQuery, val: Transaction);
}

pub fn new_file_provider(file_path: PathBuf) -> Result<Box<dyn Provider>> {
    let provider = file_provider::FileProvider::from_file(&file_path)
        .with_context(|| anyhow!("invalid cache file: {}", file_path.display()))?;

    Ok(Box::new(provider))
}

pub fn new_rpc_provider(rpc_url: String) -> Result<Box<dyn Provider>> {
    let provider = rpc_provider::RpcProvider::new(rpc_url)?;

    Ok(Box::new(provider))
}

pub fn new_cached_rpc_provider(cache_path: PathBuf, rpc_url: String) -> Result<Box<dyn Provider>> {
    let provider = cached_rpc_provider::CachedRpcProvider::new(cache_path, rpc_url)?;

    Ok(Box::new(provider))
}

pub fn new_provider(
    cache_path: Option<PathBuf>,
    rpc_url: Option<String>,
) -> Result<Box<dyn Provider>> {
    match (cache_path, rpc_url) {
        (Some(cache_path), Some(rpc_url)) => new_cached_rpc_provider(cache_path, rpc_url),
        (Some(cache_path), None) => new_file_provider(cache_path),
        (None, Some(rpc_url)) => new_rpc_provider(rpc_url),
        (None, None) => Err(anyhow!("No cache_path or rpc_url given")),
    }
}

#[cfg(feature = "taiko")]
use alloy_sol_types::TopicList;
use zeth_primitives::ethers::from_ethers_h256;

#[cfg(feature = "taiko")]
impl dyn Provider {
    pub fn filter_event_log<E: SolEvent>(
        &mut self,
        l1_contract: Address,
        l1_block_no: u64,
        l2_block_no: u64,
    ) -> Result<Vec<(Log, E)>> {
        let logs = self.get_logs(&LogsQuery {
            address: l1_contract.into_array().into(),
            from_block: l1_block_no,
            to_block: l1_block_no,
        })?;
        let res = logs
            .iter()
            .filter(|log| {
                log.topics.len() == <<E as SolEvent>::TopicList as TopicList>::COUNT
            })
            .filter(|log| from_ethers_h256(log.topics[0]) == E::SIGNATURE_HASH)
            .map(|log| {
                let topics = log.topics.iter().map(|topic| from_ethers_h256(*topic));
                let event = E::decode_raw_log(topics, &log.data, false)
                    .expect(&format!( "Decode log failed for l1_block_no {}", l1_block_no));
                (log.clone(), event)
            })
            .collect::<Vec<_>>();

        Ok(res)
    }


    fn filter_block_proposal(
        &mut self,
        l1_contract: H160,
        l1_block_no: u64,
        l2_block_no: u64,
    ) -> Result<(Transaction, BlockProposed)> {
        let logs = self.get_logs(&LogsQuery {
            address: l1_contract,
            from_block: l1_block_no,
            to_block: l1_block_no,
        })?;
        let mut res = logs
            .iter()
            .filter(|log| {
                log.topics.len() == <<BlockProposed as SolEvent>::TopicList as TopicList>::COUNT
            })
            .filter(|log| from_ethers_h256(log.topics[0]) == BlockProposed::SIGNATURE_HASH)
            .map(|log| {
                let topics = log.topics.iter().map(|topic| from_ethers_h256(*topic));
                let block_proposed = BlockProposed::decode_raw_log(topics, &log.data, false)
                    .expect(&format!(
                        "Decode log failed for l1_block_no {}",
                        l1_block_no
                    ));
                (log.block_number, log.transaction_hash, block_proposed)
            })
            .filter(|(block_no, tx_hash, event)| {
                event.blockId == revm::primitives::U256::from(l2_block_no)
            })
            .collect::<Vec<_>>();

        let (block_no, tx_hash, event) = res
            .pop()
            .with_context(|| anyhow!("Cannot find BlockProposed event for {}", l2_block_no))?;

        let tx = self
            .get_transaction(&TxQuery {
                tx_hash: tx_hash.unwrap(),
                block_no: block_no.map(|b| b.as_u64()),
            })
            .with_context(|| anyhow!("Cannot find BlockProposed Tx {:?}", tx_hash))?;

        Ok((tx, event))
    }
}


#[cfg(feature = "taiko")]
pub enum HostError {
    AlloyError(alloy_sol_types::Error),
}

#[cfg(feature = "taiko")]
impl From<HostError> for anyhow::Error {
    fn from(error: HostError) -> Self {
        anyhow!(error)
    }
}