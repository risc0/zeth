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

use std::cmp::Ordering;

use alloy_primitives::{BlockNumber, Bytes, B256};
use alloy_rlp::{Decodable, Encodable};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bytes::Buf;

pub type RawTransaction = Bytes;

#[derive(Debug, Clone, Eq, PartialEq, RlpEncodable, RlpDecodable)]
pub struct BatchEssence {
    pub parent_hash: B256,
    pub epoch_num: u64,
    pub epoch_hash: B256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Batch {
    pub inclusion_block_number: BlockNumber,
    pub essence: BatchEssence,
}

impl PartialOrd<Self> for Batch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Batch {
    fn cmp(&self, other: &Self) -> Ordering {
        self.essence.timestamp.cmp(&other.essence.timestamp)
    }
}

impl Batch {
    pub fn new(
        inclusion_block_number: BlockNumber,
        parent_hash: B256,
        epoch_num: u64,
        epoch_hash: B256,
        timestamp: u64,
    ) -> Self {
        Self {
            inclusion_block_number,
            essence: BatchEssence {
                parent_hash,
                epoch_num,
                epoch_hash,
                timestamp,
                transactions: Vec::new(),
            },
        }
    }
}

impl Encodable for Batch {
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        out.put_u8(0);
        self.essence.encode(out);
    }

    #[inline]
    fn length(&self) -> usize {
        self.essence.length() + 1
    }
}

impl Decodable for Batch {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        match buf.first() {
            Some(0) => {
                buf.advance(1);
                Ok(Self {
                    inclusion_block_number: 0,
                    essence: BatchEssence::decode(buf)?,
                })
            }
            Some(_) => Err(alloy_rlp::Error::Custom("invalid version")),
            None => Err(alloy_rlp::Error::InputTooShort),
        }
    }
}
