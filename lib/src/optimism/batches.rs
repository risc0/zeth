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

use std::{cell::RefCell, cmp::Ordering, collections::BTreeMap, io::Read};

use anyhow::Context;
use libflate::zlib::Decoder;
use zeth_primitives::{
    batch::Batch,
    rlp::{Decodable, Header},
};

use super::{channels::Channel, config::ChainConfig, derivation::State};

pub struct Batches<'a, I> {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, Batch>,
    channel_iter: I,
    state: &'a RefCell<State>,
    config: &'a ChainConfig,
}

impl<I> Iterator for Batches<'_, I>
where
    I: Iterator<Item = Channel>,
{
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        match self.try_next() {
            Ok(batch) => batch,
            Err(e) => {
                #[cfg(not(target_os = "zkvm"))]
                log::warn!("failed to decode batch: {:#}", e);
                None
            }
        }
    }
}

impl<I> Batches<'_, I> {
    pub fn new<'a>(
        channel_iter: I,
        state: &'a RefCell<State>,
        config: &'a ChainConfig,
    ) -> Batches<'a, I> {
        Batches {
            batches: BTreeMap::new(),
            channel_iter,
            state,
            config,
        }
    }
}

impl<I> Batches<'_, I>
where
    I: Iterator<Item = Channel>,
{
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
                self.batches.insert(batch.essence.timestamp, batch);
            });
        }

        let derived_batch = loop {
            if let Some((_, batch)) = self.batches.first_key_value() {
                match self.batch_status(batch) {
                    BatchStatus::Accept => {
                        let batch = batch.clone();
                        self.batches.remove(&batch.essence.timestamp);
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        #[cfg(not(target_os = "zkvm"))]
                        log::warn!("dropping invalid batch");
                        let timestamp = batch.essence.timestamp;
                        self.batches.remove(&timestamp);
                    }
                    BatchStatus::Future | BatchStatus::Undecided => {
                        break None;
                    }
                }
            } else {
                break None;
            }
        };

        let batch = if derived_batch.is_none() {
            let state = self.state.borrow();

            let current_l1_block = state.current_l1_block;
            let safe_head = state.safe_head;
            let epoch = state.epoch;
            let next_epoch = state.next_epoch;
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
        let state = self.state.borrow();

        let epoch = state.epoch;
        let next_epoch = state.next_epoch;
        let head = state.safe_head;
        let next_timestamp = head.timestamp + self.config.blocktime;

        // check timestamp range
        match batch.essence.timestamp.cmp(&next_timestamp) {
            Ordering::Greater => return BatchStatus::Future,
            Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.essence.parent_hash != head.hash {
            #[cfg(not(target_os = "zkvm"))]
            log::warn!("invalid parent hash");
            return BatchStatus::Drop;
        }

        // check the inclusion delay
        if batch.essence.epoch_num + self.config.seq_window_size < batch.l1_inclusion_block {
            #[cfg(not(target_os = "zkvm"))]
            log::warn!("inclusion window elapsed");
            return BatchStatus::Drop;
        }

        // check and set batch origin epoch
        let batch_origin = if batch.essence.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.essence.epoch_num == epoch.number + 1 {
            next_epoch
        } else {
            #[cfg(not(target_os = "zkvm"))]
            log::warn!("invalid batch origin epoch number");
            return BatchStatus::Drop;
        };

        if let Some(batch_origin) = batch_origin {
            if batch.essence.epoch_hash != batch_origin.hash {
                #[cfg(not(target_os = "zkvm"))]
                log::warn!("invalid epoch hash");
                return BatchStatus::Drop;
            }

            if batch.essence.timestamp < batch_origin.timestamp {
                #[cfg(not(target_os = "zkvm"))]
                log::warn!("batch too old");
                return BatchStatus::Drop;
            }

            // handle sequencer drift
            if batch.essence.timestamp > batch_origin.timestamp + self.config.max_seq_drift {
                if batch.essence.transactions.is_empty() {
                    if epoch.number == batch.essence.epoch_num {
                        if let Some(next_epoch) = next_epoch {
                            if batch.essence.timestamp >= next_epoch.timestamp {
                                #[cfg(not(target_os = "zkvm"))]
                                log::warn!("sequencer drift too large");
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
                    log::warn!("sequencer drift too large");
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
            log::warn!("invalid transaction");
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
