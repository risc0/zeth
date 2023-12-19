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

use std::{
    collections::{BTreeMap, VecDeque},
    io::Read,
};

use anyhow::{bail, ensure, Context, Result};
use bytes::Buf;
use libflate::zlib::Decoder;
use zeth_primitives::{
    batch::Batch,
    rlp::Decodable,
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    Address, BlockNumber,
};

use super::config::ChainConfig;

pub const MAX_RLP_BYTES_PER_CHANNEL: u64 = 10_000_000;

pub struct BatcherChannels {
    batch_inbox: Address,
    max_channel_bank_size: u64,
    channel_timeout: u64,
    channels: VecDeque<Channel>,
    batches: VecDeque<Vec<Batch>>,
}

impl BatcherChannels {
    pub fn new(config: &ChainConfig) -> Self {
        Self {
            batch_inbox: config.batch_inbox,
            max_channel_bank_size: config.max_channel_bank_size,
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

            #[cfg(not(target_os = "zkvm"))]
            log::debug!("received batcher tx: {}", tx.hash());

            for frame in Frame::process_batcher_transaction(&tx.essence)? {
                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "received frame: channel_id: {}, frame_number: {}, is_last: {}",
                    frame.channel_id,
                    frame.number,
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
                            channel.add_frame(frame).context("failed to add frame")?;
                        }
                    } else {
                        // Create new channel. From the spec:
                        // "When a channel ID referenced by a frame is not already present in the
                        //  Channel Bank, a new channel is opened, tagged with the current L1
                        //  block, and appended to the channel-queue"
                        self.channels.push_back(Channel::new(block_number, frame));
                    }
                }

                // Enforce max_channel_bank_size. From the spec:
                // "After successfully inserting a new frame, the ChannelBank is pruned: channels
                //  are dropped in FIFO order, until total_size <= MAX_CHANNEL_BANK_SIZE."
                {
                    let mut total_size = self.total_size();
                    while total_size as u64 > self.max_channel_bank_size {
                        let dropped_channel = self.channels.pop_front().unwrap();
                        total_size -= dropped_channel.size;

                        #[cfg(not(target_os = "zkvm"))]
                        log::debug!(
                            "pruned channel: {} (channel_size: {})",
                            dropped_channel.id,
                            dropped_channel.size
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

    fn total_size(&self) -> usize {
        self.channels.iter().map(|c| c.size).sum()
    }

    fn channel_index(&self, channel_id: ChannelId) -> Option<usize> {
        self.channels.iter().position(|c| c.id == channel_id)
    }
}

/// A [ChannelId] is a unique identifier for a [Channel].
type ChannelId = u128;

/// A [Channel] is a set of batches that are split into at least one, but possibly
/// multiple frames. Frames are allowed to be ingested in any order.
#[derive(Debug)]
struct Channel {
    /// The channel ID.
    id: ChannelId,
    /// The number of the L1 block that opened this channel.
    open_l1_block: u64,
    /// The number of the frame that closes this channel.
    close_frame_number: Option<u16>,
    /// All frames belonging to this channel by their frame number.
    frames: BTreeMap<u16, Frame>,
    /// The estimated memory size, used to drop the channel if we have too much data.
    size: usize,
}

impl Channel {
    const FRAME_OVERHEAD: usize = 200;

    /// Creates a new channel from the given frame.
    fn new(open_l1_block: u64, frame: Frame) -> Self {
        let mut channel = Self {
            id: frame.channel_id,
            open_l1_block,
            close_frame_number: None,
            frames: BTreeMap::new(),
            size: 0,
        };

        // cannot fail for an empty channel
        channel.add_frame(frame).unwrap();

        channel
    }

    /// Returns true if the channel is closed, i.e. the closing frame has been received.
    fn is_closed(&self) -> bool {
        self.close_frame_number.is_some()
    }

    /// Returns true if the channel is ready to be read.
    fn is_ready(&self) -> bool {
        // From the spec:
        // "A channel is ready if:
        //  - The channel is closed
        //  - The channel has a contiguous sequence of frames until the closing frame"
        matches!(self.close_frame_number, Some(n) if n as usize == self.frames.len() - 1)
    }

    fn add_frame(&mut self, frame: Frame) -> Result<()> {
        ensure!(
            frame.channel_id == self.id,
            "frame channel_id does not match channel id"
        );
        if frame.is_last && self.is_closed() {
            bail!("channel is already closed");
        }
        ensure!(
            !self.frames.contains_key(&frame.number),
            "duplicate frame number"
        );
        if let Some(close_frame_number) = self.close_frame_number {
            ensure!(
                frame.number < close_frame_number,
                "frame number >= close_frame_number"
            );
        }

        // From the spec:
        // "If a frame is closing any existing higher-numbered frames are removed from the
        // channel."
        if frame.is_last {
            // mark channel as closed
            self.close_frame_number = Some(frame.number);
            // prune frames with a number higher than the closing frame and update size
            self.frames
                .split_off(&frame.number)
                .values()
                .for_each(|pruned| self.size -= Self::FRAME_OVERHEAD + pruned.data.len());
        }

        self.size += Self::FRAME_OVERHEAD + frame.data.len();
        self.frames.insert(frame.number, frame);

        Ok(())
    }

    fn read_batches(&self, l1_block_number: BlockNumber) -> Result<Vec<Batch>> {
        let decompressed = self.decompress()?;
        let mut channel_data = decompressed.as_slice();
        let mut batches = Vec::new();

        while !channel_data.is_empty() {
            let mut batch =
                Batch::decode(&mut channel_data).context("failed to decode batch data")?;
            batch.inclusion_block_number = l1_block_number;

            batches.push(batch);
        }

        Ok(batches)
    }

    fn decompress(&self) -> Result<Vec<u8>> {
        let compressed = {
            let mut buf = Vec::new();
            for frame in self.frames.values() {
                buf.extend(&frame.data)
            }

            buf
        };

        // From the spec:
        // "When decompressing a channel, we limit the amount of decompressed data to
        //  MAX_RLP_BYTES_PER_CHANNEL (currently 10,000,000 bytes), in order to avoid "zip-bomb"
        //  types of attack (where a small compressed input decompresses to a humongous amount
        //  of data). If the decompressed data exceeds the limit, things proceeds as though the
        //  channel contained only the first MAX_RLP_BYTES_PER_CHANNEL decompressed bytes."
        let mut decompressed = Vec::new();
        Decoder::new(compressed.as_slice())?
            .take(MAX_RLP_BYTES_PER_CHANNEL)
            .read_to_end(&mut decompressed)
            .context("failed to decompress")?;

        Ok(decompressed)
    }
}

/// A [Frame] is a chunk of data belonging to a [Channel]. Batcher transactions carry one
/// or multiple frames. The reason to split a channel into frames is that a channel might
/// too large to include in a single batcher transaction.
#[derive(Debug, Default, Clone)]
struct Frame {
    /// The channel ID this frame belongs to.
    pub channel_id: ChannelId,
    /// The index of this frame within the channel.
    pub number: u16,
    /// A sequence of bytes belonging to the channel.
    pub data: Vec<u8>,
    /// Whether this is the last frame of the channel.
    pub is_last: bool,
}

impl Frame {
    const HEADER_SIZE: usize = 22;
    const MAX_FRAME_DATA_LENGTH: u32 = 1_000_000;

    /// Processes a batcher transaction and returns the list of contained frames.
    pub fn process_batcher_transaction(tx_essence: &EthereumTxEssence) -> Result<Vec<Self>> {
        let (version, mut rollup_payload) = tx_essence
            .data()
            .split_first()
            .context("empty transaction data")?;
        ensure!(version == &0, "Invalid version: {}", version);

        let mut frames = Vec::new();
        while !rollup_payload.is_empty() {
            // From the spec:
            // "If any one frame fails to parse, the all frames in the transaction are rejected."
            let frame = Frame::decode(&mut rollup_payload).context("invalid frame")?;
            frames.push(frame);
        }

        Ok(frames)
    }

    /// Decodes a [Frame] from the given buffer, advancing the buffer's position.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        ensure!(buf.remaining() > Self::HEADER_SIZE, "input too short");

        let channel_id = buf.get_u128();
        let frame_number = buf.get_u16();
        // From the spec:
        // "frame_data_length is the length of frame_data in bytes. It is capped to 1,000,000."
        let frame_data_length = buf.get_u32();
        ensure!(
            frame_data_length <= Self::MAX_FRAME_DATA_LENGTH,
            "frame_data_length too large"
        );

        let frame_data = buf
            .get(..frame_data_length as usize)
            .context("input too short")?;
        buf.advance(frame_data_length as usize);

        // From the spec:
        // "is_last is a single byte with a value of 1 if the frame is the last in the channel,
        //  0 if there are frames in the channel. Any other value makes the frame invalid."
        ensure!(buf.has_remaining(), "input too short");
        let is_last = match buf.get_u8() {
            0 => false,
            1 => true,
            _ => bail!("invalid is_last value"),
        };

        Ok(Self {
            channel_id,
            number: frame_number,
            data: frame_data.to_vec(),
            is_last,
        })
    }
}
