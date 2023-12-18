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
        // wrap the RLP-essence inside a bytes payload
        alloy_rlp::Header {
            list: false,
            payload_length: self.essence.length() + 1,
        }
        .encode(out);
        out.put_u8(0);
        self.essence.encode(out);
    }

    #[inline]
    fn length(&self) -> usize {
        let bytes_length = self.essence.length() + 1;
        alloy_rlp::length_of_length(bytes_length) + bytes_length
    }
}

impl Decodable for Batch {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let bytes = alloy_rlp::Header::decode_bytes(buf, false)?;
        match bytes.split_first() {
            Some((0, mut payload)) => Ok(Self {
                inclusion_block_number: 0,
                essence: BatchEssence::decode(&mut payload)?,
            }),
            Some(_) => Err(alloy_rlp::Error::Custom("invalid version")),
            None => Err(alloy_rlp::Error::InputTooShort),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{b256, hex::FromHex};

    use super::*;

    #[test]
    fn rlp_roundtrip() {
        let batch = Batch {
            inclusion_block_number: 0,
            essence: BatchEssence {
                parent_hash: b256!(
                    "55b11b918355b1ef9c5db810302ebad0bf2544255b530cdce90674d5887bb286"
                ),
                epoch_num: 1,
                epoch_hash: b256!(
                    "1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"
                ),
                timestamp: 1647026951,
                transactions: vec![
                    Bytes::from_hex("0x000000").unwrap(),
                    Bytes::from_hex("0x76fd7c").unwrap(),
                ],
            },
        };

        let encoded = alloy_rlp::encode(&batch);
        assert_eq!(encoded.len(), batch.length());
        let decoded = Batch::decode(&mut &encoded[..]).unwrap();
        assert_eq!(batch, decoded);
    }
}
