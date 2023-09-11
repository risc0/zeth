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

use serde::{Deserialize, Serialize};
use zeth_primitives::{BlockHash, BlockNumber, B256};

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
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Epoch {
    pub number: BlockNumber,
    pub hash: B256,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub current_l1_block_number: BlockNumber,
    pub current_l1_block_hash: BlockHash,
    pub safe_head: BlockInfo,
    pub epoch: Epoch,
    pub next_epoch: Option<Epoch>,
}
