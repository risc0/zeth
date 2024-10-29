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

use alloy::primitives::B256;
use reth_primitives::{Block, Header};
use zeth_core::stateless::data::StatelessClientData;

#[derive(Debug, Default, Clone)]
pub struct Witness {
    pub encoded_input: Vec<u8>,
    pub validated_tip: B256,
    pub validated_tail: B256,
}

impl From<StatelessClientData<Block, Header>> for Witness {
    fn from(value: StatelessClientData<Block, Header>) -> Self {
        let encoded_input = pot::to_vec(&value).expect("serialization failed");
        Self {
            encoded_input,
            validated_tip: value.blocks.last().unwrap().hash_slow(),
            validated_tail: value.parent_header.hash_slow(),
        }
    }
}
