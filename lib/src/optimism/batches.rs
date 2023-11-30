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
    io::Read,
};

use anyhow::Context;
use libflate::zlib::Decoder;
use zeth_primitives::{
    batch::Batch,
    rlp::{Decodable, Header},
    transactions::ethereum::EthereumTxEssence,
};

use super::{
    batcher_transactions::BatcherTransactions,
    channels::{Channel, Channels},
    config::ChainConfig,
    deposits,
    derivation::{Epoch, State},
    epoch::BlockInput,
};

pub struct Batches {
    /// Mapping of timestamps to batches
    batches: BinaryHeap<Reverse<Batch>>,
    pub channel_iter: Channels<BatcherTransactions>,
    pub state: State,
    pub config: ChainConfig,
}

impl Batches {
    pub fn new(config: ChainConfig, state: State) -> Batches {
        let channel_iter = Channels::new(BatcherTransactions::new(VecDeque::new()), &config);

        Batches {
            batches: BinaryHeap::new(),
            channel_iter,
            state,
            config,
        }
    }

    pub fn process(&mut self, eth_block: &BlockInput<EthereumTxEssence>) -> anyhow::Result<()> {
        let eth_block_hash = eth_block.block_header.hash();

        // Ensure block has correct parent
        if self.state.current_l1_block_number < eth_block.block_header.number {
            assert_eq!(
                eth_block.block_header.parent_hash,
                self.state.current_l1_block_hash,
            );
        }

        // Update the system config
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

        BatcherTransactions::process(
            self.config.batch_inbox,
            self.config.system_config.batch_sender,
            eth_block.block_header.number,
            &eth_block.transactions,
            &mut self.channel_iter.batcher_tx_iter.buffer,
        )?;

        self.state.current_l1_block_number = eth_block.block_header.number;
        self.state.current_l1_block_hash = eth_block_hash;

        Ok(())
    }
}

impl Iterator for Batches {
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        match self.try_next() {
            Ok(batch) => batch,
            Err(_e) => {
                #[cfg(not(target_os = "zkvm"))]
                log::warn!("failed to decode batch: {:#}", _e);
                None
            }
        }
    }
}

impl Batches {
    fn try_next(&mut self) -> anyhow::Result<Option<Batch>> {
        let channel = self.channel_iter.next();
        if let Some(channel) = channel {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!("received channel: {}", channel.id);

            let batches = decode_batches(&channel)?;
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
                        safe_head.hash,
                        epoch.number,
                        epoch.hash,
                        next_timestamp,
                        current_l1_block,
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
        if batch.essence.epoch_num + self.config.seq_window_size < batch.l1_inclusion_block {
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

fn decode_batches(channel: &Channel) -> anyhow::Result<Vec<Batch>> {
    let mut buf = Vec::new();
    let mut d = Decoder::new(channel.data.as_slice())?;
    d.read_to_end(&mut buf).context("failed to decompress")?;

    let mut batches = Vec::new();
    let mut channel_data = buf.as_slice();
    while !channel_data.is_empty() {
        let batch_data = Header::decode_bytes(&mut channel_data, false)
            .context("failed to decode batch data")?;

        let mut batch = Batch::decode(&mut &batch_data[..])?;
        batch.l1_inclusion_block = channel.l1_inclusion_block;

        batches.push(batch);
    }

    Ok(batches)
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}