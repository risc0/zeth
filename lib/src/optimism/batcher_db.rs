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

use std::collections::HashMap;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    alloy_rlp, block::Header, receipt::ReceiptEnvelope, transactions::TxEnvelope, trie::MptNode,
};

use super::{config::ChainConfig, deposits, system_config};

/// Input for extracting deposits.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockInput {
    /// Header of the block.
    pub block_header: Header,
    /// Transactions of the block.
    pub transactions: Vec<TxEnvelope>,
    /// Transaction receipts of the block or `None` if not required.
    pub receipts: Option<Vec<ReceiptEnvelope>>,
}

pub trait BatcherDb {
    fn validate(&self, config: &ChainConfig) -> Result<()>;
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput>;
    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header>;
    fn get_full_eth_block(&mut self, block_no: u64) -> Result<&BlockInput>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemDb {
    pub full_op_block: HashMap<u64, BlockInput>,
    pub op_block_header: HashMap<u64, Header>,
    pub full_eth_block: HashMap<u64, BlockInput>,
    pub eth_block_header: HashMap<u64, Header>,
}

impl MemDb {
    pub fn new() -> Self {
        MemDb {
            full_op_block: HashMap::new(),
            op_block_header: HashMap::new(),
            full_eth_block: HashMap::new(),
            eth_block_header: HashMap::new(),
        }
    }
}

impl Default for MemDb {
    fn default() -> Self {
        Self::new()
    }
}

impl BatcherDb for MemDb {
    fn validate(&self, config: &ChainConfig) -> Result<()> {
        for (block_no, op_block) in &self.full_op_block {
            let header = &op_block.block_header;
            ensure!(*block_no == header.number, "Block number mismatch");

            // Validate tx list
            {
                let mut tx_trie = MptNode::default();
                for (tx_no, tx) in op_block.transactions.iter().enumerate() {
                    tx_trie.insert_rlp(&alloy_rlp::encode(tx_no), tx)?;
                }
                ensure!(
                    tx_trie.hash() == header.transactions_root,
                    "Invalid op block transaction data!"
                );
            }

            // Validate receipts
            ensure!(
                op_block.receipts.is_none(),
                "Op blocks should not contain receipts"
            );
        }

        for (block_no, op_block) in &self.op_block_header {
            ensure!(*block_no == op_block.number, "Block number mismatch");
        }

        for (block_no, eth_block) in &self.full_eth_block {
            let header = &eth_block.block_header;
            ensure!(*block_no == header.number, "Block number mismatch");

            // Validate tx list
            {
                let mut tx_trie = MptNode::default();
                for (tx_no, tx) in eth_block.transactions.iter().enumerate() {
                    tx_trie.insert_rlp(&alloy_rlp::encode(tx_no), tx)?;
                }
                ensure!(
                    tx_trie.hash() == header.transactions_root,
                    "Invalid eth block transaction data!"
                );
            }

            // Validate receipts
            if eth_block.receipts.is_some() {
                let mut receipt_trie = MptNode::default();
                for (tx_no, receipt) in eth_block.receipts.as_ref().unwrap().iter().enumerate() {
                    receipt_trie.insert_rlp(&alloy_rlp::encode(tx_no), receipt)?;
                }
                ensure!(
                    receipt_trie.hash() == header.receipts_root,
                    "Invalid eth block receipt data!"
                );
            } else {
                let can_contain_deposits =
                    deposits::can_contain(&config.deposit_contract, &header.logs_bloom);
                let can_contain_config =
                    system_config::can_contain(&config.system_config_contract, &header.logs_bloom);
                ensure!(
                    !can_contain_deposits,
                    "Eth block has no receipts, but bloom filter indicates it has deposits"
                );
                ensure!(
                    !can_contain_config,
                    "Eth block has no receipts, but bloom filter indicates it has config updates"
                );
            }
        }

        Ok(())
    }

    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput> {
        let op_block = self.full_op_block.remove(&block_no).unwrap();

        Ok(op_block)
    }

    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header> {
        let op_block = self
            .op_block_header
            .remove(&block_no)
            .context("not or no longer in db")?;

        Ok(op_block)
    }

    fn get_full_eth_block(&mut self, block_no: u64) -> Result<&BlockInput> {
        let eth_block = self.full_eth_block.get(&block_no).unwrap();

        Ok(eth_block)
    }
}
