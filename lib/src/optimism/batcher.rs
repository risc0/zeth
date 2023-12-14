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

        // Read batches
        while let Some(batches) = self.batcher_channel.read_batches() {
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

        self.state.current_l1_block_number = eth_block.block_header.number;
        self.state.current_l1_block_hash = eth_block_hash;

        Ok(())
    }

    pub fn read_batch(&mut self) -> Result<Option<Batch>> {
        let mut out = None;

        // Grab the first accepted batch. From the spec:
        // "The batches are processed in order of the inclusion on L1: if multiple batches can be
        //  accept-ed the first is applied. An implementation can defer future batches a later
        //  derivation step to reduce validation work."
        while let Some(Reverse(batch)) = self.batches.pop() {
            match self.batch_status(&batch) {
                BatchStatus::Accept => {
                    out = Some(batch);
                    break;
                }
                BatchStatus::Drop => {
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("Dropping batch");
                }
                BatchStatus::Future => {
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("Encountered future batch");

                    self.batches.push(Reverse(batch));
                    break;
                }
                BatchStatus::Undecided => {
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("Encountered undecided batch");

                    self.batches.push(Reverse(batch));
                    break;
                }
            }
        }

        // If there are no accepted batches, attempt to generate the default batch. From the spec:
        // "If no batch can be accept-ed, and the stage has completed buffering of all batches
        //  that can fully be read from the L1 block at height epoch.number +
        // sequence_window_size,  and the next_epoch is available, then an empty batch can
        // be derived."
        if out.is_none() {
            let current_l1_block = self.state.current_l1_block_number;
            let safe_head = self.state.safe_head;
            let current_epoch = &self.state.epoch;
            let next_epoch = &self.state.next_epoch;
            let seq_window_size = self.config.seq_window_size;

            if let Some(next_epoch) = next_epoch {
                if current_l1_block > current_epoch.number + seq_window_size {
                    let next_timestamp = safe_head.timestamp + self.config.blocktime;
                    let epoch = if next_timestamp < next_epoch.timestamp {
                        // From the spec:
                        // "If next_timestamp < next_epoch.time: the current L1 origin is repeated,
                        //  to preserve the L2 time invariant."
                        current_epoch
                    } else {
                        next_epoch
                    };

                    out = Some(Batch::new(
                        current_l1_block,
                        safe_head.hash,
                        epoch.number,
                        epoch.hash,
                        next_timestamp,
                    ))
                }
            }
        }

        Ok(out)
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        // Apply the batch status rules. The spec describes a precise order for these checks.

        let epoch = &self.state.epoch;
        let next_epoch = &self.state.next_epoch;
        let safe_l2_head = self.state.safe_head;
        let next_timestamp = safe_l2_head.timestamp + self.config.blocktime;

        // From the spec:
        // "batch.timestamp > next_timestamp -> future"
        // "batch.timestamp < next_timestamp -> drop"
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
                    "Batch too old: {} = batch.essence.timestamp < next_timestamp = {}",
                    &batch.essence.timestamp,
                    &next_timestamp
                );
                return BatchStatus::Drop;
            }
            Ordering::Equal => (),
        }

        // From the spec:
        // "batch.parent_hash != safe_l2_head.hash -> drop"
        if batch.essence.parent_hash != safe_l2_head.hash {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Incorrect parent hash: {} != {}",
                batch.essence.parent_hash,
                safe_l2_head.hash
            );
            return BatchStatus::Drop;
        }

        // From the spec:
        // "batch.epoch_num + sequence_window_size < inclusion_block_number -> drop"
        if batch.essence.epoch_num + self.config.seq_window_size < batch.inclusion_block_number {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Batch is not timely: {} + {} < {}",
                batch.essence.epoch_num,
                self.config.seq_window_size,
                batch.inclusion_block_number
            );
            return BatchStatus::Drop;
        }

        // From the spec:
        // "batch.epoch_num < epoch.number -> drop"
        if batch.essence.epoch_num < epoch.number {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Batch epoch number is too low: {} < {}",
                batch.essence.epoch_num,
                epoch.number
            );
            return BatchStatus::Drop;
        }

        let batch_origin = if batch.essence.epoch_num == epoch.number {
            // From the spec:
            // "batch.epoch_num == epoch.number: define batch_origin as epoch"
            epoch
        } else if batch.essence.epoch_num == epoch.number + 1 {
            // From the spec:
            // "batch.epoch_num == epoch.number+1:"
            // "  If known, then define batch_origin as next_epoch"
            // "  If next_epoch is not known -> undecided"
            match next_epoch {
                Some(epoch) => epoch,
                None => return BatchStatus::Undecided,
            }
        } else {
            // From the spec:
            // "batch.epoch_num > epoch.number+1 -> drop"
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Batch epoch number is too large: {} > {}",
                batch.essence.epoch_num,
                epoch.number + 1
            );
            return BatchStatus::Drop;
        };

        // From the spec:
        // "batch.epoch_hash != batch_origin.hash -> drop"
        if batch.essence.epoch_hash != batch_origin.hash {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Epoch hash mismatch: {} != {}",
                batch.essence.epoch_hash,
                batch_origin.hash
            );
            return BatchStatus::Drop;
        }

        // From the spec:
        // "batch.timestamp < batch_origin.time -> drop"
        if batch.essence.timestamp < batch_origin.timestamp {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Batch violates timestamp rule: {} < {}",
                batch.essence.timestamp,
                batch_origin.timestamp
            );
            return BatchStatus::Drop;
        }

        // From the spec:
        // "batch.timestamp > batch_origin.time + max_sequencer_drift: enforce the L2 timestamp
        //  drift rule, but with exceptions to preserve above min L2 timestamp invariant:"
        if batch.essence.timestamp > batch_origin.timestamp + self.config.max_seq_drift {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "Sequencer drift detected: {} > {} + {}",
                batch.essence.timestamp,
                batch_origin.timestamp,
                self.config.max_seq_drift
            );

            // From the spec:
            // "len(batch.transactions) > 0: -> drop"
            if !batch.essence.transactions.is_empty() {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!("Sequencer drift detected for non-empty batch; drop.");
                return BatchStatus::Drop;
            }

            // From the spec:
            // "len(batch.transactions) == 0:"
            //    epoch.number == batch.epoch_num: this implies the batch does not already
            //    advance the L1 origin, and must thus be checked against next_epoch."
            if epoch.number == batch.essence.epoch_num {
                if let Some(next_epoch) = next_epoch {
                    // From the spec:
                    // "If batch.timestamp >= next_epoch.time -> drop"
                    if batch.essence.timestamp >= next_epoch.timestamp {
                        #[cfg(not(target_os = "zkvm"))]
                        log::debug!("Sequencer drift detected; drop; batch timestamp is too far into the future. {} >= {}", batch.essence.timestamp, next_epoch.timestamp);
                        return BatchStatus::Drop;
                    }
                } else {
                    // From the spec:
                    // "If next_epoch is not known -> undecided"
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("Sequencer drift detected, but next epoch is not known; undecided");
                    return BatchStatus::Undecided;
                }
            }
        }

        // From the spec:
        // "batch.transactions: drop if the batch.transactions list contains a transaction that is
        //  invalid or derived by other means exclusively:
        //    any transaction that is empty (zero length byte string)
        //    any deposited transactions (identified by the transaction type prefix byte)"
        for tx in &batch.essence.transactions {
            if matches!(tx.first(), None | Some(0x7E)) {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!("Batch contains empty or invalid transaction");
                return BatchStatus::Drop;
            }
        }

        BatchStatus::Accept
    }
}
