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

use alloy_primitives::{Bytes, ChainId, TxHash, TxNumber, B160, B256, U256};
use alloy_rlp::{Encodable, EMPTY_STRING_CODE};
use alloy_rlp_derive::RlpEncodable;
use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};

use crate::{access_list::AccessList, keccak::keccak, signature::TxSignature, RlpBytes};

/// Legacy transaction as described in [EIP-155](https://eips.ethereum.org/EIPS/eip-155).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TxEssenceLegacy {
    /// Network chain ID, added in EIP-155.
    pub chain_id: Option<ChainId>,
    /// A scalar value equal to the number of transactions sent by the sender.
    pub nonce: TxNumber,
    /// A scalar value equal to the number of Wei to be paid per unit of gas for all
    /// computation costs.
    pub gas_price: U256,
    /// A scalar value equal to the maximum amount of gas that should be used in executing
    /// this transaction.
    pub gas_limit: U256,
    /// The 160-bit address of the message call's recipient or, for a contract creation
    /// transaction, ∅.
    pub to: TransactionKind,
    /// A scalar value equal to the number of Wei to be transferred to the message call's
    /// recipient.
    pub value: U256,
    /// An unlimited size byte array specifying the transaction data.
    pub data: Bytes,
}

impl TxEssenceLegacy {
    /// Length of the RLP payload in bytes.
    fn payload_length(&self) -> usize {
        self.nonce.length()
            + self.gas_price.length()
            + self.gas_limit.length()
            + self.to.length()
            + self.value.length()
            + self.data.length()
    }

    /// Encode the transaction into the `out` buffer, only for signing.
    fn encode_signing(&self, out: &mut dyn alloy_rlp::BufMut) {
        let mut payload_length = self.payload_length();
        // if a chain ID is present, append according to EIP-155
        if let Some(chain_id) = self.chain_id {
            payload_length += chain_id.length() + 1 + 1;
        }
        alloy_rlp::Header {
            list: true,
            payload_length,
        }
        .encode(out);
        self.nonce.encode(out);
        self.gas_price.encode(out);
        self.gas_limit.encode(out);
        self.to.encode(out);
        self.value.encode(out);
        self.data.encode(out);
        if let Some(chain_id) = self.chain_id {
            chain_id.encode(out);
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
        }
    }
}

// implement Encodable to always ignore the chain ID
impl Encodable for TxEssenceLegacy {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        alloy_rlp::Header {
            list: true,
            payload_length: self.payload_length(),
        }
        .encode(out);
        self.nonce.encode(out);
        self.gas_price.encode(out);
        self.gas_limit.encode(out);
        self.to.encode(out);
        self.value.encode(out);
        self.data.encode(out);
    }

    fn length(&self) -> usize {
        let payload_length = self.payload_length();
        alloy_rlp::length_of_length(payload_length) + payload_length
    }
}

/// Transaction with an access list as described in [EIP-2930](https://eips.ethereum.org/EIPS/eip-2930).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct TxEssenceEip2930 {
    /// Network chain ID.
    pub chain_id: ChainId,
    /// A scalar value equal to the number of transactions sent by the sender.
    pub nonce: TxNumber,
    /// A scalar value equal to the number of Wei to be paid per unit of gas for all
    /// computation costs.
    pub gas_price: U256,
    /// A scalar value equal to the maximum amount of gas that should be used in executing
    /// this transaction.
    pub gas_limit: U256,
    /// The 160-bit address of the message call's recipient or, for a contract creation
    /// transaction, ∅.
    pub to: TransactionKind,
    /// A scalar value equal to the number of Wei to be transferred to the message call's
    /// recipient.
    pub value: U256,
    /// An unlimited size byte array specifying the transaction data.
    pub data: Bytes,
    /// List of access entries to warm up.
    pub access_list: AccessList,
}

/// A transaction with a priority fee as described in [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct TxEssenceEip1559 {
    /// Network chain ID, added in EIP-155.
    pub chain_id: ChainId,
    /// A scalar value equal to the number of transactions sent by the sender.
    pub nonce: TxNumber,
    /// Maximum priority fee that transaction is paying to the miner.
    pub max_priority_fee_per_gas: U256,
    /// Maximum base and priority fee paid per unit of gas for all computation costs.
    pub max_fee_per_gas: U256,
    /// A scalar value equal to the maximum amount of gas that should be used in executing
    /// this transaction.
    pub gas_limit: U256,
    /// The 160-bit address of the message call's recipient or, for a contract creation
    /// transaction, ∅.
    pub to: TransactionKind,
    /// A scalar value equal to the number of Wei to be transferred to the message call's
    /// recipient.
    pub value: U256,
    /// An unlimited size byte array specifying the transaction data.
    pub data: Bytes,
    /// List of access entries to warm up.
    pub access_list: AccessList,
}

/// Essence of a transaction, i.e. the signed part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxEssence {
    /// Legacy transaction.
    Legacy(TxEssenceLegacy),
    /// Transaction with an access list ([EIP-2930](https://eips.ethereum.org/EIPS/eip-2930)).
    Eip2930(TxEssenceEip2930),
    /// A transaction with a priority fee ([EIP-1559](https://eips.ethereum.org/EIPS/eip-1559)).
    Eip1559(TxEssenceEip1559),
}

impl Encodable for TxEssence {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            TxEssence::Legacy(tx) => tx.encode(out),
            TxEssence::Eip2930(tx) => tx.encode(out),
            TxEssence::Eip1559(tx) => tx.encode(out),
        }
    }
    fn length(&self) -> usize {
        match self {
            TxEssence::Legacy(tx) => tx.length(),
            TxEssence::Eip2930(tx) => tx.length(),
            TxEssence::Eip1559(tx) => tx.length(),
        }
    }
}

impl TxEssence {
    /// Compute the signing hash.
    pub(crate) fn signing_hash(&self) -> B256 {
        keccak(self.signing_data()).into()
    }

    fn signing_data(&self) -> Bytes {
        let mut buf = BytesMut::new();
        match self {
            TxEssence::Legacy(tx) => tx.encode_signing(&mut buf),
            TxEssence::Eip2930(tx) => {
                buf.put_u8(0x01);
                tx.encode(&mut buf);
            }
            TxEssence::Eip1559(tx) => {
                buf.put_u8(0x02);
                tx.encode(&mut buf);
            }
        };

        buf.freeze().into()
    }
}

/// Whether or not the transaction is a contract creation.
/// This cannot be an [Option] as options get RLP encoded into lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransactionKind {
    /// A contract creation transaction.
    #[default]
    Create,
    /// The 160-bit address of the transaction call's recipient.
    Call(B160),
}

impl From<TransactionKind> for Option<B160> {
    fn from(value: TransactionKind) -> Self {
        match value {
            TransactionKind::Create => None,
            TransactionKind::Call(addr) => Some(addr),
        }
    }
}

impl Encodable for TransactionKind {
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            TransactionKind::Call(addr) => addr.encode(out),
            TransactionKind::Create => out.put_u8(EMPTY_STRING_CODE),
        }
    }
    #[inline]
    fn length(&self) -> usize {
        match self {
            TransactionKind::Call(addr) => addr.length(),
            TransactionKind::Create => 1,
        }
    }
}

/// A raw transaction including the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction essence to be signed.
    pub essence: TxEssence,
    /// Signature of the transaction essence.
    pub signature: TxSignature,
}

impl Encodable for Transaction {
    /// Encodes the transaction into the `out` buffer.
    #[inline]
    fn encode(&self, out: &mut dyn BufMut) {
        // prepend the EIP-2718 transaction type
        match self.tx_type() {
            0 => {}
            tx_type => out.put_u8(tx_type),
        }

        // join the essence lists and the signature list into one
        // this allows to reuse as much of the generated RLP code as possible
        rlp_join_lists(&self.essence, &self.signature, out);
    }

    /// Length of the RLP payload in bytes.
    #[inline]
    fn length(&self) -> usize {
        let mut payload_length = self.essence.length() + self.signature.length();
        if self.tx_type() != 0 {
            payload_length += 1;
        }
        payload_length + alloy_rlp::length_of_length(payload_length)
    }
}

impl Transaction {
    /// Calculates the transaction hash.
    pub fn hash(&self) -> TxHash {
        keccak(self.to_rlp()).into()
    }

    pub fn tx_type(&self) -> u8 {
        match &self.essence {
            TxEssence::Legacy(_) => 0x00,
            TxEssence::Eip2930(_) => 0x01,
            TxEssence::Eip1559(_) => 0x02,
        }
    }
    pub fn gas_limit(&self) -> U256 {
        match &self.essence {
            TxEssence::Legacy(tx) => tx.gas_limit,
            TxEssence::Eip2930(tx) => tx.gas_limit,
            TxEssence::Eip1559(tx) => tx.gas_limit,
        }
    }
    pub fn to(&self) -> Option<B160> {
        match &self.essence {
            TxEssence::Legacy(tx) => tx.to.into(),
            TxEssence::Eip2930(tx) => tx.to.into(),
            TxEssence::Eip1559(tx) => tx.to.into(),
        }
    }
}

/// Joins two RLP-encoded lists into one lists and outputs the result into the `out`
/// buffer.
fn rlp_join_lists(a: impl Encodable, b: impl Encodable, out: &mut dyn alloy_rlp::BufMut) {
    let a_buf = alloy_rlp::encode(a);
    let header = alloy_rlp::Header::decode(&mut &a_buf[..]).unwrap();
    if !header.list {
        panic!("`a` not a list");
    }
    let a_head_length = header.length();
    let a_payload_length = a_buf.len() - a_head_length;

    let b_buf = alloy_rlp::encode(b);
    let header = alloy_rlp::Header::decode(&mut &b_buf[..]).unwrap();
    if !header.list {
        panic!("`b` not a list");
    }
    let b_head_length = header.length();
    let b_payload_length = b_buf.len() - b_head_length;

    alloy_rlp::Header {
        list: true,
        payload_length: a_payload_length + b_payload_length,
    }
    .encode(out);
    out.put_slice(&a_buf[a_head_length..]); // skip the header
    out.put_slice(&b_buf[b_head_length..]); // skip the header
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy() {
        // Tx: 0x5c504ed432cb51138bcf09aa5e8a410dd4a1e204ef84bfed1be16dfba1b22060
        let tx = json!({
                "Legacy": {
                    "nonce": 0,
                    "gas_price": "0x2d79883d2000",
                    "gas_limit": "0x5208",
                    "to": { "Call": "0x5df9b87991262f6ba471f09758cde1c0fc1de734" },
                    "value": "0x7a69",
                    "data": "0x"
                  }
        });
        let essence: TxEssence = serde_json::from_value(tx).unwrap();
        println!("signing data: {}", essence.signing_data());

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 28,
            "r": "0x88ff6cf0fefd94db46111149ae4bfc179e9b94721fffd821d38d16464b3f71d0",
            "s": "0x45e0aff800961cfce805daef7016b9b675c137a6a41a548f7b60a3484c06a33a"
        }))
        .unwrap();
        let transaction = Transaction { essence, signature };

        // verify that bincode serialization works
        let _: Transaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        assert_eq!(
            "0x5c504ed432cb51138bcf09aa5e8a410dd4a1e204ef84bfed1be16dfba1b22060",
            transaction.hash().to_string()
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            "0xa1e4380a3b1f749673e270229993ee55f35663b4",
            recovered.to_string()
        );
    }

    #[test]
    fn eip155() {
        // Tx: 0x4540eb9c46b1654c26353ac3c65e56451f711926982ce1b02f15c50e7459caf7
        let tx = json!({
                "Legacy": {
                    "nonce": 537760,
                    "gas_price": "0x03c49bfa04",
                    "gas_limit": "0x019a28",
                    "to": { "Call": "0xf0ee707731d1be239f9f482e1b2ea5384c0c426f" },
                    "value": "0x06df842eaa9fb800",
                    "data": "0x",
                    "chain_id": 1
                  }
        });
        let essence: TxEssence = serde_json::from_value(tx).unwrap();
        println!("signing data: {}", essence.signing_data());

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 38,
            "r": "0xcadd790a37b78e5613c8cf44dc3002e3d7f06a5325d045963c708efe3f9fdf7a",
            "s": "0x1f63adb9a2d5e020c6aa0ff64695e25d7d9a780ed8471abe716d2dc0bf7d4259"
        }))
        .unwrap();
        let transaction = Transaction { essence, signature };

        // verify that bincode serialization works
        let _: Transaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        assert_eq!(
            "0x4540eb9c46b1654c26353ac3c65e56451f711926982ce1b02f15c50e7459caf7",
            transaction.hash().to_string()
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            "0x974caa59e49682cda0ad2bbe82983419a2ecc400",
            recovered.to_string()
        );
    }

    #[test]
    fn eip2930() {
        // Tx: 0xbe4ef1a2244e99b1ef518aec10763b61360be22e3b649dcdf804103719b1faef
        let tx = json!({
          "Eip2930": {
            "chain_id": 1,
            "nonce": 93847,
            "gas_price": "0xf46a5a9d8",
            "gas_limit": "0x21670",
            "to": { "Call": "0xc11ce44147c9f6149fbe54adb0588523c38718d7" },
            "value": "0x10d1471",
            "data": "0x050000000002b8809aef26206090eafd7d5688615d48197d1c5ce09be6c30a33be4c861dee44d13f6dd33c2e8c5cad7e2725f88a8f0000000002d67ca5eb0e5fb6",
            "access_list": [
              {
                "address": "0xd6e64961ba13ba42858ad8a74ed9a9b051a4957d",
                "storage_keys": [
                  "0x0000000000000000000000000000000000000000000000000000000000000008",
                  "0x0b4b38935f88a7bddbe6be76893de2a04640a55799d6160729a82349aff1ffae",
                  "0xc59ee2ee2ba599569b2b1f06989dadbec5ee157c8facfe64f36a3e33c2b9d1bf"
                ]
              },
              {
                "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                "storage_keys": [
                  "0x7635825e4f8dfeb20367f8742c8aac958a66caa001d982b3a864dcc84167be80",
                  "0x42555691810bdf8f236c31de88d2cc9407a8ff86cd230ba3b7029254168df92a",
                  "0x29ece5a5f4f3e7751868475502ab752b5f5fa09010960779bf7204deb72f5dde"
                ]
              },
              {
                "address": "0x4c861dee44d13f6dd33c2e8c5cad7e2725f88a8f",
                "storage_keys": [
                  "0x000000000000000000000000000000000000000000000000000000000000000c",
                  "0x0000000000000000000000000000000000000000000000000000000000000008",
                  "0x0000000000000000000000000000000000000000000000000000000000000006",
                  "0x0000000000000000000000000000000000000000000000000000000000000007"
                ]
              },
              {
                "address": "0x90eafd7d5688615d48197d1c5ce09be6c30a33be",
                "storage_keys": [
                  "0x0000000000000000000000000000000000000000000000000000000000000001",
                  "0x9c04773acff4c5c42718bd0120c72761f458e43068a3961eb935577d1ed4effb",
                  "0x0000000000000000000000000000000000000000000000000000000000000008",
                  "0x0000000000000000000000000000000000000000000000000000000000000000",
                  "0x0000000000000000000000000000000000000000000000000000000000000004"
                ]
              }
            ]
          }
        });
        let essence: TxEssence = serde_json::from_value(tx).unwrap();
        println!("signing data: {}", essence.signing_data());

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 1,
            "r": "0xf86aa2dfde99b0d6a41741e96cfcdee0c6271febd63be4056911db19ae347e66",
            "s": "0x601deefbc4835cb15aa1af84af6436fc692dea3428d53e7ff3d34a314cefe7fc"
        }))
        .unwrap();
        let transaction = Transaction { essence, signature };

        // verify that bincode serialization works
        let _: Transaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        assert_eq!(
            "0xbe4ef1a2244e99b1ef518aec10763b61360be22e3b649dcdf804103719b1faef",
            transaction.hash().to_string()
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            "0x79b7a69d90c82e014bf0315e164208119b510fa0",
            recovered.to_string()
        );
    }

    #[test]
    fn eip1559() {
        // Tx: 0x2bcdc03343ca9c050f8dfd3c87f32db718c762ae889f56762d8d8bdb7c5d69ff
        let tx = json!({
                "Eip1559": {
                  "chain_id": 1,
                  "nonce": 32,
                  "max_priority_fee_per_gas": "0x3b9aca00",
                  "max_fee_per_gas": "0x89d5f3200",
                  "gas_limit": "0x5b04",
                  "to": { "Call": "0xa9d1e08c7793af67e9d92fe308d5697fb81d3e43" },
                  "value": "0x1dd1f234f68cde2",
                  "data": "0x",
                  "access_list": []
                }
        });
        let essence: TxEssence = serde_json::from_value(tx).unwrap();
        println!("signing data: {}", essence.signing_data());

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 0,
            "r": "0x2bdf47562da5f2a09f09cce70aed35ec9ac62f5377512b6a04cc427e0fda1f4d",
            "s": "0x28f9311b515a5f17aa3ad5ea8bafaecfb0958801f01ca11fd593097b5087121b"
        }))
        .unwrap();
        let transaction = Transaction { essence, signature };

        // verify that bincode serialization works
        let _: Transaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        assert_eq!(
            "0x2bcdc03343ca9c050f8dfd3c87f32db718c762ae889f56762d8d8bdb7c5d69ff",
            transaction.hash().to_string()
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            "0x4b9f4114d50e7907bff87728a060ce8d53bf4cf7",
            recovered.to_string()
        );
    }
}
