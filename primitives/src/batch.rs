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

extern crate alloc;
extern crate core;

pub use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
pub use core::{
    cmp::{Ord, Ordering, PartialOrd},
    convert::From,
    default::Default,
    option::{Option, Option::*},
    result::{Result, Result::*},
};

use alloy_primitives::{BlockNumber, Bytes, B256};
use alloy_rlp::{Decodable, Encodable};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use serde::{Deserialize, Serialize};
use thiserror_no_std::Error as ThisError;

/// Bytes for RLP-encoded transactions.
pub type RawTransaction = Bytes;

/// A batch represents the inputs needed to build Optimism block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch(pub BatchEssence);

/// Represents the core details of a [Batch], specifically the portion that is derived
/// from the batcher transactions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct BatchEssence {
    /// The block hash of the previous L2 block.
    pub parent_hash: B256,
    /// The number of the L1 block corresponding to the sequencing epoch of the L2 block.
    pub epoch_num: u64,
    /// The hash of the L1 block corresponding to the sequencing epoch of the L2 block.
    pub epoch_hash: B256,
    /// The timestamp of the L2 block.
    pub timestamp: u64,
    /// An RLP-encoded list of EIP-2718 encoded transactions.
    pub transactions: Vec<RawTransaction>,
}

impl Batch {
    pub fn new(parent_hash: B256, epoch_num: u64, epoch_hash: B256, timestamp: u64) -> Self {
        Batch(BatchEssence {
            parent_hash,
            epoch_num,
            epoch_hash,
            timestamp,
            transactions: Vec::new(),
        })
    }
}

impl Encodable for Batch {
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        // wrap the RLP-essence inside a bytes payload
        alloy_rlp::Header {
            list: false,
            payload_length: self.0.length() + 1,
        }
        .encode(out);
        out.put_u8(0x00);
        self.0.encode(out);
    }

    #[inline]
    fn length(&self) -> usize {
        let bytes_length = self.0.length() + 1;
        alloy_rlp::length_of_length(bytes_length) + bytes_length
    }
}

impl Decodable for Batch {
    #[inline]
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let bytes = alloy_rlp::Header::decode_bytes(buf, false)?;
        match bytes.split_first() {
            Some((0, mut payload)) => Ok(Self(BatchEssence::decode(&mut payload)?)),
            Some(_) => Err(alloy_rlp::Error::Custom("invalid version")),
            None => Err(alloy_rlp::Error::InputTooShort),
        }
    }
}

#[derive(ThisError, Debug)]
#[error(transparent)]
pub struct AlloyRlpError(#[from] alloy_rlp::Error);

#[cfg(test)]
mod tests {
    use hex_literal::hex;
    use serde_json::json;

    use super::*;

    #[test]
    fn rlp_roundtrip() {
        let expected = hex!("b85000f84da0dbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3840109d8fea0438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec0410884647f5ea9c0");
        let batch: Batch = serde_json::from_value(json!({
          "parent_hash": "0xdbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3",
          "epoch_num": 17422590,
          "epoch_hash": "0x438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108",
          "timestamp": 1686068905,
          "transactions": []
        }))
        .unwrap();

        let encoded = alloy_rlp::encode(&batch);
        assert_eq!(encoded.len(), batch.length());
        assert_eq!(encoded, expected);

        let decoded = Batch::decode(&mut &encoded[..]).unwrap();
        assert_eq!(batch, decoded);
    }
}
