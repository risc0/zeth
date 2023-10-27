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

use core::cell::RefCell;

use anyhow::{bail, ensure, Context};
use std::collections::VecDeque;
use zeth_primitives::{
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    Address, BlockNumber,
};

#[derive(Debug, Clone)]
pub struct BatcherTransactions<'a> {
    txs: VecDeque<BatcherTransaction>,
    buffer: &'a RefCell<VecDeque<BatcherTransaction>>,
}

impl BatcherTransactions<'_> {
    pub fn new(buffer: &RefCell<VecDeque<BatcherTransaction>>) -> BatcherTransactions<'_> {
        BatcherTransactions {
            txs: VecDeque::new(),
            buffer,
        }
    }

    fn drain_buffer(&mut self) {
        let mut buffer = self.buffer.borrow_mut();
        while let Some(tx) = buffer.pop_front() {
            self.txs.push_back(tx);
        }
    }
}

impl Iterator for BatcherTransactions<'_> {
    type Item = BatcherTransaction;

    fn next(&mut self) -> Option<Self::Item> {
        self.drain_buffer();
        self.txs.pop_front()
    }
}

impl BatcherTransactions<'_> {
    pub fn process(
        batch_inbox: Address,
        batch_sender: Address,
        block_number: BlockNumber,
        transactions: &Vec<Transaction<EthereumTxEssence>>,
        buffer: &RefCell<VecDeque<BatcherTransaction>>,
    ) -> anyhow::Result<()> {
        let buffer = &mut *buffer.borrow_mut();
        for tx in transactions {
            if tx.essence.to() != Some(batch_inbox) {
                continue;
            }
            if tx.recover_from()? != batch_sender {
                continue;
            }

            match BatcherTransaction::new(&tx.essence.data(), block_number) {
                Ok(batcher_tx) => {
                    buffer.push_back(batcher_tx);
                    #[cfg(not(target_os = "zkvm"))]
                    log::debug!("batcher transaction: {}", tx.hash());
                }
                Err(_e) => {
                    #[cfg(not(target_os = "zkvm"))]
                    log::warn!("invalid batcher transaction: {}", _e);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BatcherTransaction {
    pub frames: Vec<Frame>,
}

impl BatcherTransaction {
    pub fn new(data: &[u8], l1_origin: u64) -> anyhow::Result<Self> {
        let (version, mut frame_data) = data.split_first().context("invalid data")?;
        ensure!(version == &0, "Invalid version: {}", version);

        let mut frames = Vec::new();
        while !frame_data.is_empty() {
            let (frame, read) = Frame::from_data(frame_data, l1_origin)?;
            frames.push(frame);

            frame_data = &frame_data[read..];
        }

        Ok(Self { frames })
    }
}

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub channel_id: u128,
    pub frame_number: u16,
    pub frame_data_len: u32,
    pub frame_data: Vec<u8>,
    pub is_last: bool,
    pub l1_inclusion_block: u64,
}

impl Frame {
    fn from_data(data: &[u8], l1_inclusion_block: u64) -> anyhow::Result<(Self, usize)> {
        ensure!(data.len() >= 23, "invalid frame size");

        let channel_id = u128::from_be_bytes(data[0..16].try_into()?);
        let frame_number = u16::from_be_bytes(data[16..18].try_into()?);
        let frame_data_len = u32::from_be_bytes(data[18..22].try_into()?);

        let frame_data_end = 22 + frame_data_len as usize;
        ensure!(frame_data_end <= data.len(), "invalid frame size");

        let frame_data = data[22..frame_data_end].to_vec();

        let is_last = if data[frame_data_end] > 1 {
            bail!("invalid is_last flag");
        } else {
            data[frame_data_end] != 0
        };

        let frame = Self {
            channel_id,
            frame_number,
            frame_data_len,
            frame_data,
            is_last,
            l1_inclusion_block,
        };

        Ok((frame, data.len()))
    }
}
