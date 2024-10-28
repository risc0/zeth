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

use alloy::primitives::U256;
use alloy::rpc::types::{Block, Header};
use zeth_core::stateless::driver::SCEDriver;

#[derive(Default, Copy, Clone, Debug)]
pub struct AlloyDriver;

impl SCEDriver<Block, Header> for AlloyDriver {
    fn block_to_header(block: Block) -> Header {
        block.header
    }

    fn accumulate_difficulty(total_difficulty: U256, header: &Header) -> U256 {
        total_difficulty + header.difficulty
    }
}