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
#[cfg(not(feature = "std"))]
use crate::no_std_preflight::*;

use alloy_primitives::{Address, Bloom, BloomInput, Bytes, B256, U256};
use alloy_rlp::Encodable;
use alloy_rlp_derive::RlpEncodable;
use serde::{Deserialize, Serialize};

/// Represents an Ethereum log entry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct Log {
    /// Contract that emitted this log.
    pub address: Address,
    /// Topics of the log. The number of logs depend on what `LOG` opcode is used.
    pub topics: Vec<B256>,
    /// Arbitrary length data.
    pub data: Bytes,
}

/// Payload of a [Receipt].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct ReceiptPayload {
    /// Indicates whether the transaction was executed successfully.
    pub success: bool,
    /// Total gas used by the transaction.
    pub cumulative_gas_used: U256,
    /// A bloom filter that contains indexed information of logs for quick searching.
    pub logs_bloom: Bloom,
    /// Logs generated during the execution of the transaction.
    pub logs: Vec<Log>,
}

/// Receipt containing result of transaction execution.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Receipt {
    /// Type of Receipt.
    pub tx_type: u8,
    /// Detailed payload of the receipt.
    pub payload: ReceiptPayload,
}

impl Encodable for Receipt {
    /// Encodes the receipt into the `out` buffer.
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self.tx_type {
            // For legacy transactions
            0 => self.payload.encode(out),
            // For EIP-2718 typed transactions
            tx_type => {
                // prepend the EIP-2718 transaction type
                out.put_u8(tx_type);
                // append the RLP-encoded payload
                self.payload.encode(out);
            }
        }
    }

    /// Returns the length of the encoded receipt in bytes.
    #[inline]
    fn length(&self) -> usize {
        let mut payload_length = self.payload.length();
        if self.tx_type != 0 {
            payload_length += 1;
        }
        payload_length
    }
}

impl Receipt {
    /// Constructs a new [Receipt].
    ///
    /// This function also computes the `logs_bloom` based on the provided logs.
    pub fn new(tx_type: u8, success: bool, cumulative_gas_used: U256, logs: Vec<Log>) -> Receipt {
        let mut logs_bloom = Bloom::default();
        for log in &logs {
            logs_bloom.accrue(BloomInput::Raw(log.address.as_slice()));
            for topic in &log.topics {
                logs_bloom.accrue(BloomInput::Raw(topic.as_slice()));
            }
        }

        Receipt {
            tx_type,
            payload: ReceiptPayload {
                success,
                cumulative_gas_used,
                logs_bloom,
                logs,
            },
        }
    }
}

// test vectors from https://github.com/ethereum/go-ethereum/blob/c40ab6af72ce282020d03c33e8273ea9b03d58f6/core/types/receipt_test.go
#[cfg(test)]
mod tests {
    use hex_literal::hex;
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy() {
        let expected = hex!("f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt::new(
            0,
            false,
            U256::from(1),
            serde_json::from_value(json!([
                {
                    "address": "0x0000000000000000000000000000000000000011",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                },
                {
                    "address": "0x0000000000000000000000000000000000000111",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                }
            ]))
            .unwrap(),
        );
        let mut data = vec![];
        receipt.encode(&mut data);

        assert_eq!(data, expected);
    }

    #[test]
    fn eip2930() {
        let expected = hex!("01f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt::new(
            1,
            false,
            U256::from(1),
            serde_json::from_value(json!([
                {
                    "address": "0x0000000000000000000000000000000000000011",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                },
                {
                    "address": "0x0000000000000000000000000000000000000111",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                }
            ]))
            .unwrap(),
        );
        let mut data = vec![];
        receipt.encode(&mut data);

        assert_eq!(data, expected);
    }

    #[test]
    fn eip1559() {
        let expected = hex!("02f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt::new(
            2,
            false,
            U256::from(1),
            serde_json::from_value(json!([
                {
                    "address": "0x0000000000000000000000000000000000000011",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                },
                {
                    "address": "0x0000000000000000000000000000000000000111",
                    "topics": [
                        "0x000000000000000000000000000000000000000000000000000000000000dead",
                        "0x000000000000000000000000000000000000000000000000000000000000beef"
                    ],
                    "data": "0x0100ff"
                }
            ]))
            .unwrap(),
        );
        let mut data = vec![];
        receipt.encode(&mut data);

        assert_eq!(data, expected);
    }
}
