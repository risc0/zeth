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

use core::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    batch::{Batch, BatchEssence},
    transactions::{
        ethereum::EthereumTxEssence,
        optimism::{OptimismTxEssence, OPTIMISM_DEPOSITED_TX_TYPE},
        Transaction,
    },
    BlockHash, BlockNumber, U256,
};

use super::{
    batcher_channel::BatcherChannels, batcher_db::BlockInput, config::ChainConfig, deposits,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize, Ord, PartialOrd)]
pub struct BlockId {
    pub hash: BlockHash,
    pub number: BlockNumber,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct L2BlockInfo {
    pub hash: BlockHash,
    pub timestamp: u64,
    pub l1_origin: BlockId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Epoch {
    pub number: BlockNumber,
    pub hash: BlockHash,
    pub timestamp: u64,
    pub base_fee_per_gas: U256,
    pub deposits: Vec<Transaction<OptimismTxEssence>>,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub current_l1_block_number: BlockNumber,
    pub current_l1_block_hash: BlockHash,
    pub safe_head: L2BlockInfo,
    pub epoch: Epoch,
    pub op_epoch_queue: VecDeque<Epoch>,
    pub next_epoch: Option<Epoch>,
}

impl State {
    pub fn new(
        current_l1_block_number: BlockNumber,
        current_l1_block_hash: BlockHash,
        safe_head: L2BlockInfo,
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

    pub fn do_next_epoch(&mut self) -> Result<()> {
        self.epoch = self.next_epoch.take().context("no next epoch!")?;
        self.deque_next_epoch_if_none()?;
        Ok(())
    }

    pub fn push_epoch(&mut self, epoch: Epoch) -> Result<()> {
        self.op_epoch_queue.push_back(epoch);
        self.deque_next_epoch_if_none()?;
        Ok(())
    }

    fn deque_next_epoch_if_none(&mut self) -> Result<()> {
        if self.next_epoch.is_none() {
            while let Some(next_epoch) = self.op_epoch_queue.pop_front() {
                if next_epoch.number <= self.epoch.number {
                    continue;
                } else if next_epoch.number == self.epoch.number + 1 {
                    self.next_epoch = Some(next_epoch);
                    break;
                } else {
                    bail!("Epoch gap!");
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

/// A [Batch] with inclusion information.
pub struct BatchWithInclusion {
    pub essence: BatchEssence,
    pub inclusion_block_number: BlockNumber,
}

pub struct Batcher {
    /// Multimap of batches, keyed by timestamp
    batches: BTreeMap<u64, VecDeque<BatchWithInclusion>>,
    batcher_channel: BatcherChannels,
    pub state: State,
    pub config: ChainConfig,
}

impl Batcher {
    pub fn new(
        config: ChainConfig,
        op_head: L2BlockInfo,
        eth_block: &BlockInput<EthereumTxEssence>,
    ) -> Result<Batcher> {
        let eth_block_hash = eth_block.block_header.hash();

        let batcher_channel = BatcherChannels::new(&config);

        let state = State::new(
            eth_block.block_header.number,
            eth_block_hash,
            op_head,
            Epoch {
                number: eth_block.block_header.number,
                hash: eth_block_hash,
                timestamp: eth_block.block_header.timestamp.try_into().unwrap(),
                base_fee_per_gas: eth_block.block_header.base_fee_per_gas,
                deposits: deposits::extract_transactions(&config, eth_block)?,
            },
        );

        Ok(Batcher {
            batches: BTreeMap::new(),
            batcher_channel,
            state,
            config,
        })
    }

    pub fn process_l1_block(&mut self, eth_block: &BlockInput<EthereumTxEssence>) -> Result<()> {
        let eth_block_hash = eth_block.block_header.hash();

        // Ensure block has correct parent
        ensure!(
            eth_block.block_header.parent_hash == self.state.current_l1_block_hash,
            "Eth block has invalid parent hash"
        );

        if eth_block.receipts.is_some() {
            // Update the system config. From the spec:
            // "Upon traversal of the L1 block, the system configuration copy used by the L1
            //  retrieval stage is updated, such that the batch-sender authentication is always
            //  accurate to the exact L1 block that is read by the stage"
            self.config
                .system_config
                .update(&self.config.system_config_contract, eth_block)
                .context("failed to update system config")?;

            // update the specification from L1 and not from L2
            let header = &eth_block.block_header;
            self.config.update_spec_id(&header.timestamp);
        }

        // Enqueue epoch
        self.state.push_epoch(Epoch {
            number: eth_block.block_header.number,
            hash: eth_block_hash,
            timestamp: eth_block.block_header.timestamp.try_into().unwrap(),
            base_fee_per_gas: eth_block.block_header.base_fee_per_gas,
            deposits: deposits::extract_transactions(&self.config, eth_block)?,
        })?;

        // process all transactions of this block to generate batches
        self.batcher_channel
            .process_l1_transactions(
                self.config.system_config.batch_sender,
                eth_block.block_header.number,
                &eth_block.transactions,
            )
            .context("failed to process transactions")?;

        // Read batches
        while let Some(batches) = self.batcher_channel.read_batches() {
            batches.into_iter().for_each(|batch| {
                #[cfg(not(target_os = "zkvm"))]
                log::trace!(
                    "received batch: timestamp={}, parent_hash={}, epoch={}",
                    batch.essence.timestamp,
                    batch.essence.parent_hash,
                    batch.essence.epoch_num
                );
                self.batches
                    .entry(batch.essence.timestamp)
                    .or_default()
                    .push_back(batch);
            });
        }

        self.state.current_l1_block_number = eth_block.block_header.number;
        self.state.current_l1_block_hash = eth_block_hash;

        Ok(())
    }

    pub fn read_batch(&mut self) -> Result<Option<Batch>> {
        let epoch = &self.state.epoch;
        let safe_l2_head = self.state.safe_head;

        ensure!(
            safe_l2_head.l1_origin.hash == epoch.hash
                || safe_l2_head.l1_origin.number == epoch.number - 1,
            "buffered L1 chain epoch does not match safe head origin"
        );

        let mut next_batch = None;

        // Grab the first accepted batch. From the spec:
        // "The batches are processed in order of the inclusion on L1: if multiple batches can be
        //  accept-ed the first is applied. An implementation can defer future batches a later
        //  derivation step to reduce validation work."
        'outer: while let Some((ts, mut batches)) = self.batches.pop_first() {
            // iterate over all batches, in order of inclusion and find the first accepted batch
            // retain batches that may be processed in the future, or those we are undecided on
            while let Some(batch) = batches.pop_front() {
                match self.batch_status(&batch) {
                    BatchStatus::Accept => {
                        next_batch = Some(batch);
                        // if there are still batches left, insert them back into the map
                        if !batches.is_empty() {
                            self.batches.insert(ts, batches);
                        }
                        break 'outer;
                    }
                    BatchStatus::Drop => {}
                    BatchStatus::Future | BatchStatus::Undecided => {
                        batches.push_front(batch);
                        self.batches.insert(ts, batches);
                        break 'outer;
                    }
                }
            }
        }

        if let Some(batch) = next_batch {
            return Ok(Some(Batch(batch.essence)));
        }

        // If there are no accepted batches, attempt to generate the default batch. From the spec:
        // "If no batch can be accept-ed, and the stage has completed buffering of all batches
        //  that can fully be read from the L1 block at height epoch.number +
        //  sequence_window_size, and the next_epoch is available, then an empty batch can be
        //  derived."
        let current_l1_block = self.state.current_l1_block_number;
        let sequence_window_size = self.config.seq_window_size;
        let first_of_epoch = epoch.number == safe_l2_head.l1_origin.number + 1;

        if current_l1_block > epoch.number + sequence_window_size {
            if let Some(next_epoch) = &self.state.next_epoch {
                let next_timestamp = safe_l2_head.timestamp + self.config.blocktime;
                let batch_epoch = if next_timestamp < next_epoch.timestamp || first_of_epoch {
                    // From the spec:
                    // "If next_timestamp < next_epoch.time: the current L1 origin is repeated,
                    //  to preserve the L2 time invariant."
                    // "If the batch is the first batch of the epoch, that epoch is used instead
                    //  of advancing the epoch to ensure that there is at least one L2 block per
                    //  epoch."
                    epoch
                } else {
                    next_epoch
                };

                return Ok(Some(Batch::new(
                    safe_l2_head.hash,
                    batch_epoch.number,
                    batch_epoch.hash,
                    next_timestamp,
                )));
            }
        }

        Ok(None)
    }

    fn batch_status(&self, batch: &BatchWithInclusion) -> BatchStatus {
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
                log::trace!(
                    "Future batch: {} = batch.timestamp > next_timestamp = {}",
                    &batch.essence.timestamp,
                    &next_timestamp
                );
                return BatchStatus::Future;
            }
            Ordering::Less => {
                #[cfg(not(target_os = "zkvm"))]
                log::trace!(
                    "Batch too old: {} = batch.timestamp < next_timestamp = {}",
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
            log::warn!(
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
            log::warn!(
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
            log::warn!(
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
            log::warn!(
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
            log::warn!(
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
            log::warn!(
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
                log::warn!("Sequencer drift detected for non-empty batch; drop.");
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
                        log::warn!("Sequencer drift detected; drop; batch timestamp is too far into the future. {} >= {}", batch.essence.timestamp, next_epoch.timestamp);
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
            if matches!(tx.first(), None | Some(&OPTIMISM_DEPOSITED_TX_TYPE)) {
                #[cfg(not(target_os = "zkvm"))]
                log::warn!("Batch contains empty or invalid transaction");
                return BatchStatus::Drop;
            }
        }

        BatchStatus::Accept
    }
}
