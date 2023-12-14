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

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeth_primitives::{
    block::Header,
    receipt::Receipt,
    transactions::{
        ethereum::EthereumTxEssence, optimism::OptimismTxEssence, Transaction, TxEssence,
    },
    trie::MptNode,
    RlpBytes,
};

use crate::optimism::{config::OPTIMISM_CHAIN_SPEC, deposits, system_config};

/// Input for extracting deposits.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockInput<E: TxEssence> {
    /// Header of the block.
    pub block_header: Header,
    /// Transactions of the block.
    pub transactions: Vec<Transaction<E>>,
    /// Transaction receipts of the block or `None` if not required.
    pub receipts: Option<Vec<Receipt>>,
}

pub trait BatcherDb {
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput<OptimismTxEssence>>;
    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header>;
    fn get_full_eth_block(&mut self, block_no: u64) -> Result<BlockInput<EthereumTxEssence>>;
    fn get_eth_block_header(&mut self, block_no: u64) -> Result<Header>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemDb {
    pub full_op_block: HashMap<u64, BlockInput<OptimismTxEssence>>,
    pub op_block_header: HashMap<u64, Header>,
    pub full_eth_block: HashMap<u64, BlockInput<EthereumTxEssence>>,
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
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput<OptimismTxEssence>> {
        let op_block = self.full_op_block.remove(&block_no).unwrap();
        assert_eq!(block_no, op_block.block_header.number);

        // Validate tx list
        {
            let mut tx_trie = MptNode::default();
            for (tx_no, tx) in op_block.transactions.iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                tx_trie.insert_rlp(&trie_key, tx)?;
            }
            if tx_trie.hash() != op_block.block_header.transactions_root {
                bail!("Invalid op block transaction data!")
            }
        }

        Ok(op_block)
    }

    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header> {
        let op_block = self.op_block_header.remove(&block_no).unwrap();
        assert_eq!(block_no, op_block.number);

        Ok(op_block)
    }

    fn get_full_eth_block(&mut self, block_no: u64) -> Result<BlockInput<EthereumTxEssence>> {
        let eth_block = self.full_eth_block.remove(&block_no).unwrap();
        assert_eq!(block_no, eth_block.block_header.number);

        // Validate tx list
        {
            let mut tx_trie = MptNode::default();
            for (tx_no, tx) in eth_block.transactions.iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                tx_trie.insert_rlp(&trie_key, tx)?;
            }
            if tx_trie.hash() != eth_block.block_header.transactions_root {
                bail!("Invalid eth block transaction data!")
            }
        }

        // Validate receipts
        if eth_block.receipts.is_some() {
            let mut receipt_trie = MptNode::default();
            for (tx_no, receipt) in eth_block.receipts.as_ref().unwrap().iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                receipt_trie.insert_rlp(&trie_key, receipt)?;
            }
            if receipt_trie.hash() != eth_block.block_header.receipts_root {
                bail!("Invalid eth block receipt data!")
            }
        } else {
            let can_contain_deposits = deposits::can_contain(
                &OPTIMISM_CHAIN_SPEC.deposit_contract,
                &eth_block.block_header.logs_bloom,
            );
            let can_contain_config = system_config::can_contain(
                &OPTIMISM_CHAIN_SPEC.system_config_contract,
                &eth_block.block_header.logs_bloom,
            );
            assert!(!can_contain_deposits);
            assert!(!can_contain_config);
        }

        Ok(eth_block)
    }

    fn get_eth_block_header(&mut self, block_no: u64) -> Result<Header> {
        let eth_block = self.eth_block_header.remove(&block_no).unwrap();
        assert_eq!(block_no, eth_block.number);

        Ok(eth_block)
    }
}
