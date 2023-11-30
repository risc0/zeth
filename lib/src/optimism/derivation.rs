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

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use zeth_primitives::{
    transactions::{optimism::OptimismTxEssence, Transaction},
    BlockHash, BlockNumber, B256, U256,
};

use super::config::ChainConfig;

pub const CHAIN_SPEC: ChainConfig = ChainConfig::optimism();

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: B256,
    // pub parent_hash: B256,
    pub timestamp: u64,
}

/// L1 epoch block
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
