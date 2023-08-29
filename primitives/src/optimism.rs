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

use alloy_primitives::{Address, Bytes, TxHash};
use alloy_rlp::{Encodable, EMPTY_STRING_CODE};
use serde::{Deserialize, Serialize};

use crate::transaction::TransactionKind;

// use reth_codecs::{main_codec, Compact};
// use reth_rlp::{length_of_length, Decodable, DecodeError, Encodable, Header,
// EMPTY_STRING_CODE};

/// Deposit transactions, also known as deposits, are initiated on L1, and executed on L2.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TxDeposit {
    /// Hash that uniquely identifies the source of the deposit.
    pub source_hash: TxHash,
    /// The address of the sender account.
    pub from: Address,
    /// The address of the recipient account, or the null (zero-length) address if the
    /// deposited transaction is a contract creation.
    pub to: TransactionKind,
    /// The ETH value to mint on L2.
    pub mint: Option<u128>,
    ///  The ETH value to send to the recipient account.
    pub value: u128,
    /// The gas limit for the L2 transaction.
    pub gas_limit: u64,
    /// Field indicating if this transaction is exempt from the L2 gas limit.
    pub is_system_transaction: bool,
    /// Input has two uses depending if transaction is Create or Call (if `to` field is
    /// None or Some).
    pub input: Bytes,
}

impl TxDeposit {
    /// Computes the length of the RLP-encoded payload in bytes.
    ///
    /// This method calculates the combined length of all the individual fields
    /// of the transaction when they are RLP-encoded.
    pub(crate) fn payload_length(&self) -> usize {
        let mut len = 0;
        len += self.source_hash.length();
        len += self.from.length();
        len += self.to.length();
        len += self.mint.map_or(1, |mint| mint.length());
        len += self.value.length();
        len += self.gas_limit.length();
        len += self.is_system_transaction.length();
        len += self.input.0.length();
        len
    }

    /// Encodes the transaction into the provided `out` buffer for the purpose of signing.
    pub(crate) fn signing_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.encode(out);
    }

    /// Computes the length of the RLP-encoded transaction essence in bytes for signing.
    ///
    /// This method calculates the total length of the transaction when it is RLP-encoded,
    /// including any additional bytes required for the encoding format.
    pub(crate) fn signing_length(&self) -> usize {
        let payload_length = self.payload_length();
        // 'tx type' + 'header length' + 'payload length'
        let len = 1 + alloy_rlp::length_of_length(payload_length) + payload_length;
        alloy_rlp::length_of_length(len) + len
    }

    /// Encodes only the transaction's fields into the desired buffer, without a RLP
    /// header. <https://github.com/ethereum-optimism/optimism/blob/develop/specs/deposits.md#the-deposited-transaction-type>
    pub(crate) fn encode_fields(&self, out: &mut dyn bytes::BufMut) {
        self.source_hash.encode(out);
        self.from.encode(out);
        self.to.encode(out);
        if let Some(mint) = self.mint {
            mint.encode(out);
        } else {
            out.put_u8(EMPTY_STRING_CODE);
        }
        self.value.encode(out);
        self.gas_limit.encode(out);
        self.is_system_transaction.encode(out);
        self.input.encode(out);
    }

    /// Get the transaction type
    pub(crate) fn tx_type(&self) -> u8 {
        0x7e
    }
}

// Implement the Encodable trait for `TxDeposit`.
impl Encodable for TxDeposit {
    /// Encodes the [TxDeposit] instance into the provided `out` buffer.
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        let payload_length = self.payload_length();
        // if with_header {
        //     alloy_rlp::Header {
        //         list: false,
        //         payload_length: 1 + alloy_rlp::length_of_length(payload_length) +
        // payload_length,     }
        //     .encode(out);
        // }
        out.put_u8(self.tx_type());
        let header = alloy_rlp::Header {
            list: true,
            payload_length,
        };
        header.encode(out);
        self.encode_fields(out);
    }

    /// Computes the length of the RLP-encoded [TxDeposit] instance in bytes.
    ///
    /// This method calculates the total length of the transaction when it is RLP-encoded.
    fn length(&self) -> usize {
        let payload_length = self.payload_length();
        // 'tx type' + 'header length' + 'payload length'
        let len = 1 + alloy_rlp::length_of_length(payload_length) + payload_length;
        alloy_rlp::length_of_length(len) + len
    }
}

// #[cfg(test)]
// mod tests {
//     use crate::{Bytes, TransactionSigned};
//     use bytes::BytesMut;
//     use reth_rlp::Decodable;
//     use revm_primitives::hex_literal::hex;
//
//     #[test]
//     fn test_rlp_roundtrip() {
//         let bytes =
// hex!("7ef9015aa044bae9d41b8380d781187b426c6fe43df5fb2fb57bd4466ef6a701e1f01e015694deaddeaddeaddeaddeaddeaddeaddeaddead000194420000000000000000000000000000000000001580808408f0d18001b90104015d8eb900000000000000000000000000000000000000000000000000000000008057650000000000000000000000000000000000000000000000000000000063d96d10000000000000000000000000000000000000000000000000000000000009f35273d89754a1e0387b89520d989d3be9c37c1f32495a88faf1ea05c61121ab0d1900000000000000000000000000000000000000000000000000000000000000010000000000000000000000002d679b567db6187c0c8323fa982cfb88b74dbcc7000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240"
// );
//
//         let tx_a =
// TransactionSigned::decode_enveloped(Bytes::from(&bytes[..])).unwrap();         let tx_b
// = TransactionSigned::decode(&mut &bytes[..]).unwrap();
//
//         let mut buf_a = BytesMut::default();
//         tx_a.encode_enveloped(&mut buf_a);
//         assert_eq!(&buf_a[..], &bytes[..]);
//
//         let mut buf_b = BytesMut::default();
//         tx_b.encode_enveloped(&mut buf_b);
//         assert_eq!(&buf_b[..], &bytes[..]);
//     }
// }
