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

use core::cmp::Ordering;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

use anyhow::{Context, Result};
use zeth_primitives::{
    batch::Batch,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence, Transaction},
    BlockHash, BlockNumber, B256, U256,
};

use super::{
    batcher_channel::BatcherChannels, batcher_db::BlockInput, config::ChainConfig, deposits,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct BlockInfo {
    pub hash: B256,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Epoch {
    pub number: BlockNumber,
    pub hash: B256,
    pub timestamp: u64,
    pub base_fee_per_gas: U256,
    pub deposits: Vec<Transaction<OptimismTxEssence>>,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub current_l1_block_number: BlockNumber,
    pub current_l1_block_hash: BlockHash,
    pub safe_head: BlockInfo,
    pub epoch: Epoch,
    pub op_epoch_queue: VecDeque<Epoch>,
    pub next_epoch: Option<Epoch>,
}

impl State {
    pub fn new(
        current_l1_block_number: BlockNumber,
        current_l1_block_hash: BlockHash,
        safe_head: BlockInfo,
        epoch: Epoch,
    ) -> Self {
        State {
            current_l1_block_number,
            current_l1_block_hash,
            safe_head,
            epoch,
            op_epoch_queue: VecDeque::new(),
            next_epoch: None,
        }
    }

    pub fn do_next_epoch(&mut self) -> anyhow::Result<()> {
        self.epoch = self.next_epoch.take().expect("No next epoch!");
        self.deque_next_epoch_if_none()?;
        Ok(())
    }

    pub fn push_epoch(&mut self, epoch: Epoch) -> anyhow::Result<()> {
        self.op_epoch_queue.push_back(epoch);
        self.deque_next_epoch_if_none()?;
        Ok(())
    }

    fn deque_next_epoch_if_none(&mut self) -> anyhow::Result<()> {
        if self.next_epoch.is_none() {
            while let Some(next_epoch) = self.op_epoch_queue.pop_front() {
                if next_epoch.number <= self.epoch.number {
                    continue;
                } else if next_epoch.number == self.epoch.number + 1 {
                    self.next_epoch = Some(next_epoch);
                    break;
                } else {
                    anyhow::bail!("Epoch gap!");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}

pub struct Batcher {
    batches: BinaryHeap<Reverse<Batch>>,
    batcher_channel: BatcherChannels,
    pub state: State,
    pub config: ChainConfig,
}

impl Batcher {
    pub fn new(config: ChainConfig, state: State) -> Batcher {
        let batcher_channel = BatcherChannels::new(&config);

        Batcher {
            batches: BinaryHeap::new(),
            batcher_channel,
            state,
            config,
        }
    }

    pub fn process_l1_block(&mut self, eth_block: &BlockInput<EthereumTxEssence>) -> Result<()> {
        let eth_block_hash = eth_block.block_header.hash();

        // Ensure block has correct parent
        if self.state.current_l1_block_number < eth_block.block_header.number {
            assert_eq!(
                eth_block.block_header.parent_hash,
                self.state.current_l1_block_hash,
            );
        }

        // Update the system config. From the spec:
        // "Upon traversal of the L1 block, the system configuration copy used by the L1 retrieval
        //  stage is updated, such that the batch-sender authentication is always accurate to the
        //  exact L1 block that is read by the stage"
        if eth_block.receipts.is_some() {
            self.config
                .system_config
                .update(&self.config.system_config_contract, &eth_block)
                .context("failed to update system config")?;
        }

        // Enqueue epoch
        self.state.push_epoch(Epoch {
            number: eth_block.block_header.number,
            hash: eth_block_hash,
            timestamp: eth_block.block_header.timestamp.try_into().unwrap(),
            base_fee_per_gas: eth_block.block_header.base_fee_per_gas,
            deposits: deposits::extract_transactions(&self.config, &eth_block)?,
        })?;

        // Read frames into channels
        self.batcher_channel.process_l1_transactions(
            self.config.system_config.batch_sender,
            eth_block.block_header.number,
            &eth_block.transactions,
        )?;

        self.state.current_l1_block_number = eth_block.block_header.number;
        self.state.current_l1_block_hash = eth_block_hash;

        Ok(())
    }

    pub fn read_batch(&mut self) -> Result<Option<Batch>> {
        if let Some(batches) = self.batcher_channel.read_batches() {
            batches.into_iter().for_each(|batch| {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "saw batch: t={}, ph={:?}, e={}",
                    batch.essence.timestamp,
                    batch.essence.parent_hash,
                    batch.essence.epoch_num
                );
                self.batches.push(Reverse(batch));
            });
        }

        let derived_batch = loop {
            if let Some(Reverse(batch)) = self.batches.pop() {
                match self.batch_status(&batch) {
                    BatchStatus::Accept => {
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        #[cfg(not(target_os = "zkvm"))]
                        log::debug!("Dropping invalid batch");
                    }
                    BatchStatus::Future | BatchStatus::Undecided => {
                        self.batches.push(Reverse(batch));
                        break None;
                    }
                }
            } else {
                break None;
            }
        };

        let batch = if derived_batch.is_none() {
            let current_l1_block = self.state.current_l1_block_number;
            let safe_head = self.state.safe_head;
            let epoch = &self.state.epoch;
            let next_epoch = &self.state.next_epoch;
            let seq_window_size = self.config.seq_window_size;

            if let Some(next_epoch) = next_epoch {
                if current_l1_block > epoch.number + seq_window_size {
                    let next_timestamp = safe_head.timestamp + self.config.blocktime;
                    let epoch = if next_timestamp < next_epoch.timestamp {
                        epoch
                    } else {
                        next_epoch
                    };

                    Some(Batch::new(
                        current_l1_block,
                        safe_head.hash,
                        epoch.number,
                        epoch.hash,
                        next_timestamp,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            derived_batch.to_owned()
        };

        Ok(batch)
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        let epoch = &self.state.epoch;
        let next_epoch = &self.state.next_epoch;
        let head = self.state.safe_head;
        let next_timestamp = head.timestamp + self.config.blocktime;

        // check timestamp range
        match batch.essence.timestamp.cmp(&next_timestamp) {
            Ordering::Greater => {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "Future batch: {} = batch.essence.timestamp > next_timestamp = {}",
                    &batch.essence.timestamp,
                    &next_timestamp
                );
                return BatchStatus::Future;
            }
            Ordering::Less => {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "Drop batch: {} = batch.essence.timestamp < next_timestamp = {}",
                    &batch.essence.timestamp,
                    &next_timestamp
                );
                return BatchStatus::Drop;
            }
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.essence.parent_hash != head.hash {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("invalid parent hash");
            return BatchStatus::Drop;
        }

        // check the inclusion delay
        if batch.essence.epoch_num + self.config.seq_window_size < batch.inclusion_block_number {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("inclusion window elapsed");
            return BatchStatus::Drop;
        }

        // check and set batch origin epoch
        let batch_origin = if batch.essence.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.essence.epoch_num == epoch.number + 1 {
            next_epoch.as_ref()
        } else {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("invalid batch origin epoch number");
            return BatchStatus::Drop;
        };

        if let Some(batch_origin) = batch_origin {
            if batch.essence.epoch_hash != batch_origin.hash {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!("invalid epoch hash");
                return BatchStatus::Drop;
            }

            if batch.essence.timestamp < batch_origin.timestamp {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!("batch too old");
                return BatchStatus::Drop;
            }

            // handle sequencer drift
            if batch.essence.timestamp > batch_origin.timestamp + self.config.max_seq_drift {
                if batch.essence.transactions.is_empty() {
                    if epoch.number == batch.essence.epoch_num {
                        if let Some(next_epoch) = next_epoch {
                            if batch.essence.timestamp >= next_epoch.timestamp {
                                #[cfg(not(target_os = "zkvm"))]
                                log::debug!("sequencer drift too large");
                                return BatchStatus::Drop;
                            }
                        } else {
                            #[cfg(not(target_os = "zkvm"))]
                            log::debug!("sequencer drift undecided");
                            return BatchStatus::Undecided;
                        }
                    }
                } else {
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("sequencer drift too large");
                    return BatchStatus::Drop;
                }
            }
        } else {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("batch origin not known");
            return BatchStatus::Undecided;
        }

        if batch
            .essence
            .transactions
            .iter()
            .any(|tx| matches!(tx.first(), None | Some(0x7E)))
        {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("invalid transaction");
            return BatchStatus::Drop;
        }

        BatchStatus::Accept
    }
}
