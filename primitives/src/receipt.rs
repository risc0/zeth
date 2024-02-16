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

pub use alloy_consensus::{Receipt, ReceiptWithBloom};
use alloy_eips::eip2718::{Decodable2718, Eip2718Error, Encodable2718};
use alloy_network::Receipt as _;
use alloy_primitives::{Bloom, Log, TxNumber};
use alloy_rlp::{Decodable, Encodable};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{transactions::TxType, RlpBytes};

/// Represents a minimal EVM transaction receipt.
pub trait EvmReceipt: Encodable + Decodable {
    /// Returns the receipt's success status.
    fn success(&self) -> bool;
    /// Returns the receipt's cumulative gas used.
    fn cumulative_gas_used(&self) -> u64;
    /// Returns the receipt's logs.
    fn logs(&self) -> &[Log];
    /// Returns the receipt's logs bloom.
    fn logs_bloom(&self) -> &Bloom;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptEnvelope {
    /// Receipt envelope with no type flag.
    Legacy(ReceiptWithBloom),
    /// Receipt envelope with type flag 1, containing a [EIP-2930] receipt.
    Eip2930(ReceiptWithBloom),
    /// Receipt envelope with type flag 2, containing a [EIP-1559] receipt.
    Eip1559(ReceiptWithBloom),
    /// Receipt envelope with type flag 2, containing a [EIP-4844] receipt.
    Eip4844(ReceiptWithBloom),
    /// Receipt envelope with type flag 0x7e, containing a [OptimismDepositReceipt].
    OptimismDeposit(OptimismDepositReceipt),
}

impl ReceiptEnvelope {
    /// Return the [`TxType`] of the inner txn.
    pub const fn tx_type(&self) -> TxType {
        match self {
            Self::Legacy(_) => TxType::Legacy,
            Self::Eip2930(_) => TxType::Eip2930,
            Self::Eip1559(_) => TxType::Eip1559,
            Self::Eip4844(_) => TxType::Eip4844,
            Self::OptimismDeposit(_) => TxType::OptimismDeposit,
        }
    }

    fn inner_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            Self::Legacy(t) => t.encode(out),
            Self::Eip2930(t) => t.encode(out),
            Self::Eip1559(t) => t.encode(out),
            Self::Eip4844(t) => t.encode(out),
            Self::OptimismDeposit(t) => t.encode(out),
        }
    }

    fn inner_length(&self) -> usize {
        match self {
            Self::Legacy(t) => t.length(),
            Self::Eip2930(t) => t.length(),
            Self::Eip1559(t) => t.length(),
            Self::Eip4844(t) => t.length(),
            Self::OptimismDeposit(t) => t.length(),
        }
    }
}

impl EvmReceipt for ReceiptEnvelope {
    fn success(&self) -> bool {
        match self {
            Self::Legacy(r) => r.success(),
            Self::Eip2930(r) => r.success(),
            Self::Eip1559(r) => r.success(),
            Self::Eip4844(r) => r.success(),
            Self::OptimismDeposit(r) => r.success,
        }
    }

    fn cumulative_gas_used(&self) -> u64 {
        match self {
            Self::Legacy(r) => r.cumulative_gas_used(),
            Self::Eip2930(r) => r.cumulative_gas_used(),
            Self::Eip1559(r) => r.cumulative_gas_used(),
            Self::Eip4844(r) => r.cumulative_gas_used(),
            Self::OptimismDeposit(r) => r.cumulative_gas_used,
        }
    }

    fn logs(&self) -> &[Log] {
        match self {
            Self::Legacy(r) => r.logs(),
            Self::Eip2930(r) => r.logs(),
            Self::Eip1559(r) => r.logs(),
            Self::Eip4844(r) => r.logs(),
            Self::OptimismDeposit(r) => &r.logs,
        }
    }

    fn logs_bloom(&self) -> &Bloom {
        match self {
            Self::Legacy(r) => &r.bloom,
            Self::Eip2930(r) => &r.bloom,
            Self::Eip1559(r) => &r.bloom,
            Self::Eip4844(r) => &r.bloom,
            Self::OptimismDeposit(r) => &r.logs_bloom,
        }
    }
}

impl Serialize for ReceiptEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bytes = alloy_rlp::encode(self);
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReceiptEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes = <Vec<u8>>::deserialize(deserializer)?;
        Self::decode_bytes(bytes).map_err(serde::de::Error::custom)
    }
}

/// Encodes the receipt following the EIP-2718 standard.
impl Encodable for ReceiptEnvelope {
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.encode_2718(out);
    }
    #[inline]
    fn length(&self) -> usize {
        self.encode_2718_len()
    }
}

/// Decodes the receipt following the EIP-2718 standard.
impl Decodable for ReceiptEnvelope {
    #[inline]
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        match Self::decode_2718(buf) {
            Ok(tx) => Ok(tx),
            Err(Eip2718Error::RlpError(e)) => Err(e),
            Err(_) => Err(alloy_rlp::Error::Custom("Unexpected type")),
        }
    }
}

impl Encodable2718 for ReceiptEnvelope {
    fn type_flag(&self) -> Option<u8> {
        match self {
            Self::Legacy(_) => None,
            Self::Eip2930(_) => Some(TxType::Eip2930 as u8),
            Self::Eip1559(_) => Some(TxType::Eip1559 as u8),
            Self::Eip4844(_) => Some(TxType::Eip4844 as u8),
            Self::OptimismDeposit(_) => Some(TxType::OptimismDeposit as u8),
        }
    }

    fn encode_2718_len(&self) -> usize {
        match self {
            Self::Legacy(tx) => tx.length(),
            _ => 1 + self.inner_length(),
        }
    }

    fn encode_2718(&self, out: &mut dyn bytes::BufMut) {
        match self {
            Self::Legacy(tx) => tx.encode(out),
            _ => {
                out.put_u8(self.tx_type() as u8);
                self.inner_encode(out);
            }
        }
    }
}

impl Decodable2718 for ReceiptEnvelope {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Result<Self, Eip2718Error> {
        match ty.try_into()? {
            TxType::Legacy => unreachable!(),
            TxType::Eip2930 => Ok(Self::Eip2930(Decodable::decode(buf)?)),
            TxType::Eip1559 => Ok(Self::Eip1559(Decodable::decode(buf)?)),
            TxType::Eip4844 => Ok(Self::Eip4844(Decodable::decode(buf)?)),
            TxType::OptimismDeposit => Ok(Self::OptimismDeposit(Decodable::decode(buf)?)),
        }
    }

    fn fallback_decode(buf: &mut &[u8]) -> Result<Self, Eip2718Error> {
        Ok(Self::Legacy(Decodable::decode(buf)?))
    }
}

/// Version of the deposit nonce field in the receipt.
const OPTIMISM_DEPOSIT_NONCE_VERSION: usize = 1;

/// Receipt containing result of an Optimism Deposit transaction execution.
/// The Deposit transaction receipt type is equal to a regular receipt, but extended with
/// an optional `depositNonce` field.
#[derive(Clone, Debug, PartialEq, Eq, Default, RlpEncodable, RlpDecodable)]
#[rlp(trailing)]
pub struct OptimismDepositReceipt {
    /// Indicates whether the transaction was executed successfully.
    pub success: bool,
    /// Total gas used by the transaction.
    pub cumulative_gas_used: u64,
    /// A bloom filter that contains indexed information of logs for quick searching.
    pub logs_bloom: Bloom,
    /// Logs generated during the execution of the transaction.
    pub logs: Vec<Log>,
    /// Nonce of the Optimism deposit transaction persisted during execution.
    pub deposit_nonce: Option<TxNumber>,
    /// Version of the deposit nonce field in the receipt.
    pub deposit_nonce_version: Option<usize>,
}

impl OptimismDepositReceipt {
    /// Constructs a new [OptimismDepositReceipt].
    /// With Canyon, the deposit nonce must be supplied.
    pub fn new(receipt: ReceiptWithBloom, deposit_nonce: Option<TxNumber>) -> Self {
        Self {
            success: receipt.receipt.success,
            cumulative_gas_used: receipt.receipt.cumulative_gas_used,
            logs_bloom: receipt.bloom,
            logs: receipt.receipt.logs,
            deposit_nonce,
            deposit_nonce_version: deposit_nonce.map(|_| OPTIMISM_DEPOSIT_NONCE_VERSION),
        }
    }
}

// test vectors from https://github.com/ethereum/go-ethereum/blob/c40ab6af72ce282020d03c33e8273ea9b03d58f6/core/types/receipt_test.go
#[cfg(test)]
mod tests {
    use alloy_consensus::Receipt;
    use hex_literal::hex;
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy() {
        let expected = hex!("f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt {
            success: false,
            cumulative_gas_used: 1,
            logs: serde_json::from_value(json!([
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
        };
        let envelop = ReceiptEnvelope::Legacy(receipt.into());
        assert_eq!(envelop.encoded_2718(), expected);
    }

    #[test]
    fn eip2930() {
        let expected = hex!("01f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt {
            success: false,
            cumulative_gas_used: 1,
            logs: serde_json::from_value(json!([
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
        };
        let envelop = ReceiptEnvelope::Eip2930(receipt.into());
        assert_eq!(envelop.encoded_2718(), expected);
    }

    #[test]
    fn eip1559() {
        let expected = hex!("02f901c58001b9010000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000000000000000010000080000000000000000000004000000000000000000000000000040000000000000000000000000000800000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000f8bef85d940000000000000000000000000000000000000011f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100fff85d940000000000000000000000000000000000000111f842a0000000000000000000000000000000000000000000000000000000000000deada0000000000000000000000000000000000000000000000000000000000000beef830100ff");
        let receipt = Receipt {
            success: false,
            cumulative_gas_used: 1,
            logs: serde_json::from_value(json!([
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
        };
        let envelop = ReceiptEnvelope::Eip1559(receipt.into());
        assert_eq!(envelop.encoded_2718(), expected);
    }
}
