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

use std::{collections::VecDeque, io::Read};

use anyhow::{ensure, Context, Result};
use libflate::zlib::Decoder;
use zeth_primitives::{
    batch::Batch,
    rlp::{Decodable, Header},
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    Address, BlockNumber,
};

use super::config::ChainConfig;

pub struct BatcherChannels {
    batch_inbox: Address,
    max_channel_size: u64,
    channel_timeout: u64,
    channels: VecDeque<Channel>,
    batches: VecDeque<Vec<Batch>>,
}

impl BatcherChannels {
    pub fn new(config: &ChainConfig) -> Self {
        Self {
            batch_inbox: config.batch_inbox,
            max_channel_size: config.max_channel_size,
            channel_timeout: config.channel_timeout,
            channels: VecDeque::new(),
            batches: VecDeque::new(),
        }
    }

    pub fn process_l1_transactions(
        &mut self,
        batch_sender: Address,
        block_number: BlockNumber,
        transactions: &Vec<Transaction<EthereumTxEssence>>,
    ) -> anyhow::Result<()> {
        for tx in transactions {
            // From the spec:
            // "The receiver must be the configured batcher inbox address."
            if tx.essence.to() != Some(self.batch_inbox) {
                continue;
            }
            // From the spec:
            // "The sender must match the batcher address loaded from the system config matching
            //  the L1 block of the data."
            if tx.recover_from()? != batch_sender {
                continue;
            }

            for frame in Frame::process_l1_transaction(&tx.essence)? {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "received frame: channel_id: {}, frame_number: {}, is_last: {}",
                    frame.channel_id,
                    frame.frame_number,
                    frame.is_last
                );

                let frame_channel_id = frame.channel_id;

                // Send the frame to its corresponding channel
                {
                    if let Some(channel_index) = self.channel_index(frame_channel_id) {
                        let channel = &mut self.channels[channel_index];

                        // Enforce channel_timeout
                        if block_number > channel.open_l1_block + self.channel_timeout {
                            // Remove the channel. From the spec:
                            // "New frames for timed-out channels are dropped instead of buffered."
                            self.channels.remove(channel_index);
                        } else {
                            // Add frame to channel
                            channel.process_frame(frame);
                        }
                    } else {
                        // Create new channel. From the spec:
                        // "When a channel ID referenced by a frame is not already present in the
                        //  Channel Bank, a new channel is opened, tagged with the current L1
                        //  block, and appended to the channel-queue"
                        self.channels.push_back(Channel::new(block_number, frame));
                    }
                }

                // Enforce max_channel_size. From the spec:
                // "After successfully inserting a new frame, the ChannelBank is pruned: channels
                //  are dropped in FIFO order, until total_size <= MAX_CHANNEL_BANK_SIZE."
                {
                    while self.total_frame_data_len() as u64 > self.max_channel_size {
                        let _dropped_channel = self.channels.pop_front().unwrap();

                        #[cfg(not(target_os = "zkvm"))]
                        log::debug!(
                            "dropped channel: {} (frames_data_len: {})",
                            _dropped_channel.id,
                            _dropped_channel.frames_data_len
                        );
                    }
                }

                // Decode batches from channel (if ready)
                if let Some(channel_index) = self.channel_index(frame_channel_id) {
                    if self.channels[channel_index].is_ready() {
                        let channel = self.channels.remove(channel_index).unwrap();

                        #[cfg(not(target_os = "zkvm"))]
                        log::debug!("received channel: {}", channel.id);

                        self.batches.push_back(channel.read_batches(block_number)?);
                    }
                }
            }
        }

        Ok(())
    }

    pub fn read_batches(&mut self) -> Option<Vec<Batch>> {
        self.batches.pop_front()
    }

    fn total_frame_data_len(&self) -> usize {
        let mut out = 0;
        for channel in &self.channels {
            out += channel.frames_data_len;
        }
        out
    }

    fn channel_index(&self, channel_id: u128) -> Option<usize> {
        self.channels.iter().position(|c| c.id == channel_id)
    }
}

#[derive(Debug)]
struct Channel {
    id: u128,
    open_l1_block: u64,
    // From the spec:
    // "the sum of all buffered frame data of the channel, with an additional frame-overhead of
    //  200 bytes per frame."
    frames_data_len: usize,
    frames: Vec<Frame>,
    expected_frames_len: Option<usize>,
}

impl Channel {
    const FRAME_OVERHEAD: usize = 200;

    fn new(open_l1_block: u64, frame: Frame) -> Self {
        let expected_frames_len = if frame.is_last {
            Some(frame.frame_number as usize + 1)
        } else {
            None
        };

        Self {
            id: frame.channel_id,
            open_l1_block,
            frames_data_len: Self::FRAME_OVERHEAD + frame.frame_data.len(),
            frames: vec![frame],
            expected_frames_len,
        }
    }

    fn contains(&self, frame_number: u16) -> bool {
        self.frames
            .iter()
            .any(|existing_frame| existing_frame.frame_number == frame_number)
    }

    fn process_frame(&mut self, frame: Frame) {
        // From the spec:
        // "Duplicate frames (by frame number) for frames that have not been pruned from the
        //  channel-bank are dropped."
        if self.contains(frame.frame_number) {
            #[cfg(not(target_os = "zkvm"))]
            log::debug!(
                "channel {} dropping duplicate frame {}",
                self.id,
                frame.frame_number
            );

            return;
        }

        // From the spec:
        // "Duplicate closes (new frame is_last == 1, but the channel has already seen a closing
        //  frame and has not yet been pruned from the channel-bank) are dropped."
        if frame.is_last {
            if self.expected_frames_len.is_some() {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "channel {} dropping duplicate close-frame {}",
                    self.id,
                    frame.frame_number
                );

                return;
            }
            self.expected_frames_len = Some(frame.frame_number as usize + 1);
        }

        self.frames_data_len += Self::FRAME_OVERHEAD + frame.frame_data.len();
        self.frames.push(frame);
    }

    fn read_batches(&self, l1_block_number: BlockNumber) -> Result<Vec<Batch>> {
        let decompressed = self.decompress()?;
        let mut channel_data = decompressed.as_slice();
        let mut batches = Vec::new();

        while !channel_data.is_empty() {
            let batch_data = Header::decode_bytes(&mut channel_data, false)
                .context("failed to decode batch data")?;

            let mut batch = Batch::decode(&mut &batch_data[..])?;
            batch.inclusion_block_number = l1_block_number;

            batches.push(batch);
        }

        Ok(batches)
    }

    fn is_ready(&self) -> bool {
        self.expected_frames_len == Some(self.frames.len())
    }

    fn decompress(&self) -> Result<Vec<u8>> {
        let compressed = {
            let mut buf = Vec::new();

            let mut sorted_frames: Vec<&Frame> = self.frames.iter().collect();
            sorted_frames.sort_by_key(|f| f.frame_number);

            for frame in sorted_frames {
                buf.extend(&frame.frame_data)
            }

            buf
        };

        let mut decompressed = Vec::new();
        Decoder::new(compressed.as_slice())?
            .read_to_end(&mut decompressed)
            .context("failed to decompress")?;

        Ok(decompressed)
    }
}

#[derive(Debug, Default, Clone)]
struct Frame {
    pub channel_id: u128,
    pub frame_number: u16,
    pub frame_data: Vec<u8>,
    pub is_last: bool,
}

impl Frame {
    const HEADER_SIZE: usize = 22;

    pub fn process_l1_transaction(tx_essence: &EthereumTxEssence) -> Result<Vec<Self>> {
        let (version, mut frame_data) = tx_essence.data().split_first().context("invalid data")?;
        ensure!(version == &0, "Invalid version: {}", version);

        let mut frames = Vec::new();
        while !frame_data.is_empty() {
            let frame = Frame::parse(frame_data)?;
            frame_data = {
                let bytes_read = Self::HEADER_SIZE + frame.frame_data.len() + 1;
                &frame_data[bytes_read..]
            };
            frames.push(frame);
        }

        Ok(frames)
    }

    fn parse(data: &[u8]) -> Result<Self> {
        ensure!(Self::HEADER_SIZE < data.len(), "Insufficient frame data");

        let channel_id = u128::from_be_bytes(data[0..16].try_into()?);
        let frame_number = u16::from_be_bytes(data[16..18].try_into()?);
        let frame_data_len = u32::from_be_bytes(data[18..22].try_into()?);

        let frame_data_end = Self::HEADER_SIZE + frame_data_len as usize;
        ensure!(frame_data_end <= data.len(), "frame_data_end too large");
        ensure!(data[frame_data_end] <= 1, "Invalid byte at frame_data_end");

        let frame_data = data[22..frame_data_end].to_vec();
        let is_last = data[frame_data_end] != 0;

        let frame = Self {
            channel_id,
            frame_number,
            frame_data,
            is_last,
        };

        Ok(frame)
    }
}
