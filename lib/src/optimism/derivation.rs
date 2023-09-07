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

use std::{cell::RefCell, collections::VecDeque};

use anyhow::{ensure, Context, Ok};
use serde::{Deserialize, Serialize};
use zeth_primitives::{block::Header, keccak256, trie::MptNode, BlockNumber, RlpBytes, B256};

use super::{
    batcher_transactions::BatcherTransactions,
    batches::Batches,
    channels::Channels,
    config::ChainConfig,
    deposits,
    epoch::{Input, Output},
};
use crate::optimism::{batcher_transactions::BatcherTransaction, epoch::BlockInput};

pub const CHAIN_SPEC: ChainConfig = ChainConfig::optimism();

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: B256,
    // pub parent_hash: B256,
    pub timestamp: u64,
}

/// L1 epoch block
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Epoch {
    pub number: BlockNumber,
    pub hash: B256,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct State {
    pub current_l1_block: BlockNumber,
    pub safe_head: BlockInfo,
    pub epoch: Epoch,
    pub next_epoch: Option<Epoch>,
}

pub struct Deriver {
    config: ChainConfig,
    l2_head: BlockInfo,
}

impl Deriver {
    pub fn new(config: ChainConfig, l2_head: BlockInfo) -> Self {
        Self { config, l2_head }
    }

    pub fn derive(self, input: Input) -> anyhow::Result<Output> {
        let first = input.first().context("no input blocks")?;
        let epoch_number = first.block_header.number;

        // initialize the derivation state based on the first block
        let state = RefCell::new(State {
            current_l1_block: first.block_header.number,
            safe_head: self.l2_head,
            epoch: Epoch {
                number: first.block_header.number,
                hash: first.block_header.hash(),
                timestamp: first.block_header.timestamp.try_into().unwrap(),
            },
            next_epoch: None,
        });

        let buffer = RefCell::new(VecDeque::new());

        // setup the pipeline
        let batcher_transactions = BatcherTransactions::new(&buffer);
        let channels = Channels::new(batcher_transactions, &self.config);
        let mut batches = Batches::new(channels, &state, &self.config);

        // extract deposits from the first block
        let _deposits = deposits::extract_hashes(&self.config, first)
            .context("failed to extract deposit hashes")?;

        let mut system_config = self.config.system_config.clone();

        // check the correctness of the receipts if present
        if let Some(receipts) = &first.receipts {
            let mut receipts_trie = MptNode::default();
            for (idx, receipt) in receipts.into_iter().enumerate() {
                receipts_trie.insert_rlp(&idx.to_rlp(), receipt)?;
            }
            ensure!(
                receipts_trie.hash() == first.block_header.receipts_root,
                "receipts root mismatch"
            );
        }

        let mut l2_batch_hashes = Vec::new();

        let mut prev: Option<B256> = None;
        for block_input in input {
            // assure that the blocks form a chain
            if let Some(prev) = prev {
                ensure!(
                    block_input.block_header.parent_hash == prev,
                    "parent hash mismatch"
                );
            }
            prev = Some(block_input.block_header.hash());

            #[cfg(not(target_os = "zkvm"))]
            log::debug!("processing block: {}", block_input.block_header.number);

            // update the derivation state
            {
                let mut state = state.borrow_mut();
                state.current_l1_block = block_input.block_header.number;
                if state.next_epoch.is_none() {
                    state.next_epoch = Some(Epoch {
                        number: block_input.block_header.number,
                        hash: block_input.block_header.hash(),
                        timestamp: block_input.block_header.timestamp.try_into().unwrap(),
                    });
                }
            }

            // update the system config
            system_config
                .update(&self.config, &block_input)
                .context("failed to update system config")?;

            // process all batcher transactions
            BatcherTransactions::process(
                self.config.batch_inbox,
                system_config.batch_sender,
                block_input.block_header.number,
                &block_input.transactions,
                &buffer,
            )
            .context("failed to create batcher transactions")?;

            // verify transactions trie
            {
                let mut txs_trie = MptNode::default();
                for (idx, tx) in block_input.transactions.into_iter().enumerate() {
                    txs_trie.insert_rlp(&idx.to_rlp(), tx)?;
                }
                ensure!(
                    txs_trie.hash() == block_input.block_header.transactions_root,
                    "receipts root mismatch"
                );
            }

            // extract ready batches
            while let Some(batch) = batches.next() {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "derived batch: t={}, ph={:?}, e={}",
                    batch.essence.timestamp,
                    batch.essence.parent_hash,
                    batch.essence.epoch_num
                );

                if batch.essence.epoch_num == epoch_number {
                    let mut state = state.borrow_mut();
                    // TODO: build the actual block from that batch and update the state

                    l2_batch_hashes.push(keccak256(batch.to_rlp()));
                }
            }
        }

        Ok(Output {
            l1_block_hash: prev.unwrap(),
            l2_block_hashes: l2_batch_hashes,
        })
    }
}

pub struct DynamicDeriver<'a, 'b> {
    batches: Batches<'b, Channels<BatcherTransactions<'a>>>,
}

impl<'a, 'b> DynamicDeriver<'a, 'b> {
    pub fn new(
        config: &'b ChainConfig,
        state: &'b RefCell<State>,
        buffer: &'a RefCell<VecDeque<BatcherTransaction>>,
    ) -> Self {
        Self {
            batches: Batches::new(
                Channels::new(BatcherTransactions::new(buffer), config),
                state,
                config,
            ),
        }
    }

    pub fn derive(&mut self, input: BlockInput) -> anyhow::Result<Header> {
        todo!()
    }
}
