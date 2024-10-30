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

use alloy_consensus::Header;
use alloy_primitives::{B256, U256};
use reth_primitives::Block;

pub trait SCEDriver<Block, Header>: Default {
    fn header_hash(header: &Header) -> B256;
    fn block_header(block: &Block) -> &Header;
    fn block_to_header(block: Block) -> Header;
    fn accumulate_difficulty(total_difficulty: U256, header: &Header) -> U256;
}

#[derive(Default, Copy, Clone, Debug)]
pub struct RethDriver;

impl SCEDriver<Block, Header> for RethDriver {
    fn header_hash(header: &Header) -> B256 {
        header.hash_slow()
    }

    fn block_header(block: &Block) -> &Header {
        &block.header
    }

    fn block_to_header(block: Block) -> Header {
        block.header
    }

    fn accumulate_difficulty(total_difficulty: U256, header: &Header) -> U256 {
        total_difficulty + header.difficulty
    }
}
