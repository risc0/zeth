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

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_rlp::{Decodable, Encodable};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bytes::{Buf, BufMut};
use k256::ecdsa::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::signature::TxSignature;
use crate::transactions::{
    ethereum::{EthereumTxEssence, TransactionKind},
    SignedDecodable, TxEssence,
};

/// The EIP-2718 transaction type for an Optimism deposited transaction.
pub const OPTIMISM_DEPOSITED_TX_TYPE: u8 = 0x7E;

/// Represents an Optimism depositing transaction that is a L2 transaction that was
/// derived from L1 and included in a L2 block.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable, RlpDecodable,
)]
pub struct TxEssenceOptimismDeposited {
    /// The source hash which uniquely identifies the origin of the deposit.
    pub source_hash: B256,
    /// The 160-bit address of the sender.
    pub from: Address,
    /// The 160-bit address of the intended recipient for a message call or
    /// [TransactionKind::Create] for contract creation.
    pub to: TransactionKind,
    /// The ETH value to mint on L2.
    pub mint: U256,
    /// The amount, in Wei, to be transferred to the recipient of the message call.
    pub value: U256,
    /// The maximum amount of gas allocated for the execution of the L2 transaction.
    pub gas_limit: U256,
    /// If true, the transaction does not interact with the L2 block gas pool.
    pub is_system_tx: bool,
    /// The transaction's payload, represented as a variable-length byte array.
    pub data: Bytes,
}

/// Represents the core essence of an Optimism transaction, specifically the portion that
/// gets signed.
///
/// The [OptimismTxEssence] enum provides a way to handle different types of Optimism
/// transactions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimismTxEssence {
    /// Represents an Ethereum-compatible L2 transaction.
    Ethereum(EthereumTxEssence),
    /// Represents an Optimism depositing transaction.
    OptimismDeposited(TxEssenceOptimismDeposited),
}

impl Encodable for OptimismTxEssence {
    /// Encodes the [OptimismTxEssence] enum variant into the provided `out` buffer.
    #[inline]
    fn encode(&self, out: &mut dyn BufMut) {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.encode(out),
            OptimismTxEssence::OptimismDeposited(op) => op.encode(out),
        }
    }

    /// Computes the length of the RLP-encoded [OptimismTxEssence] enum variant in bytes.
    #[inline]
    fn length(&self) -> usize {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.length(),
            OptimismTxEssence::OptimismDeposited(op) => op.length(),
        }
    }
}

impl SignedDecodable<TxSignature> for OptimismTxEssence {
    fn decode_signed(buf: &mut &[u8]) -> alloy_rlp::Result<(Self, TxSignature)> {
        match buf.first().copied() {
            Some(0x7e) => {
                buf.advance(1);
                Ok((
                    OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited::decode(buf)?),
                    TxSignature::default(),
                ))
            }
            Some(_) => EthereumTxEssence::decode_signed(buf)
                .map(|(e, s)| (OptimismTxEssence::Ethereum(e), s)),
            None => Err(alloy_rlp::Error::InputTooShort),
        }
    }
}

impl TxEssence for OptimismTxEssence {
    /// Returns the EIP-2718 transaction type.
    fn tx_type(&self) -> u8 {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.tx_type(),
            OptimismTxEssence::OptimismDeposited(_) => OPTIMISM_DEPOSITED_TX_TYPE,
        }
    }
    /// Returns the gas limit set for the transaction.
    fn gas_limit(&self) -> U256 {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.gas_limit(),
            OptimismTxEssence::OptimismDeposited(op) => op.gas_limit,
        }
    }
    /// Returns the recipient address of the transaction, if available.
    fn to(&self) -> Option<Address> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.to(),
            OptimismTxEssence::OptimismDeposited(op) => op.to.into(),
        }
    }
    /// Recovers the Ethereum address of the sender from the transaction's signature
    /// using the provided verification key.
    fn recover_with_vk(
        &self,
        signature: &TxSignature,
        verify_key: &VerifyingKey,
    ) -> anyhow::Result<Address> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.recover_with_vk(signature, verify_key),
            OptimismTxEssence::OptimismDeposited(op) => Ok(op.from),
        }
    }
    /// Recovers the ECDSA verification key of this transaction's signer
    fn verifying_key(&self, signature: &TxSignature) -> anyhow::Result<VerifyingKey> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.verifying_key(signature),
            OptimismTxEssence::OptimismDeposited(_) => anyhow::bail!("Undefined!"),
        }
    }
    /// Recovers the Ethereum address of the sender from the transaction's signature.
    fn recover_from(&self, signature: &TxSignature) -> anyhow::Result<Address> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.recover_from(signature),
            OptimismTxEssence::OptimismDeposited(op) => Ok(op.from),
        }
    }
    /// Returns the length of the RLP-encoding payload in bytes.
    fn payload_length(&self) -> usize {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.payload_length(),
            OptimismTxEssence::OptimismDeposited(op) => op._alloy_rlp_payload_length(),
        }
    }
    /// Returns a reference to the transaction's call data
    fn data(&self) -> &Bytes {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.data(),
            OptimismTxEssence::OptimismDeposited(op) => &op.data,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use serde_json::json;

    use super::*;
    use crate::{
        transactions::{OptimismTransaction, Transaction},
        RlpBytes,
    };

    #[test]
    fn ethereum() {
        // Tx: 0x9125dcdf2a82f349bbcd8c1201cc601b7b4f98975c76d1f8ee3ce9270334fb8a
        let tx = json!({
            "Ethereum": {
                "Eip1559": {
                  "chain_id": 10,
                  "nonce": 17,
                  "max_priority_fee_per_gas": "0x3b9cdd02",
                  "max_fee_per_gas": "0x3b9cdd02",
                  "gas_limit": "0x01a1a7",
                  "to": { "Call": "0x7f5c764cbc14f9669b88837ca1490cca17c31607" },
                  "value": "0x0",
                  "data": "0x095ea7b30000000000000000000000004c5d5234f232bd2d76b96aa33f5ae4fcf0e4bfabffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                  "access_list": []
                }
            }
        });
        let essence: OptimismTxEssence = serde_json::from_value(tx).unwrap();

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 0,
            "r": "0x044e091fe419b233ddc76c616f60f33f5c68a5d6ea315b0b22afdbe5af66b9e6",
            "s": "0x7f00ccbff42777c6c2e5d5e85579a7984783be446cc1b9a7b8e080d167d56fa8"
        }))
        .unwrap();

        let transaction = OptimismTransaction { essence, signature };

        // verify the RLP roundtrip
        let decoded = Transaction::decode_bytes(alloy_rlp::encode(&transaction)).unwrap();
        assert_eq!(transaction, decoded);

        // verify that bincode serialization works
        let _: OptimismTransaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        let encoded = alloy_rlp::encode(&transaction);
        assert_eq!(encoded.len(), transaction.length());

        assert_eq!(
            transaction.hash(),
            b256!("9125dcdf2a82f349bbcd8c1201cc601b7b4f98975c76d1f8ee3ce9270334fb8a")
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            recovered,
            address!("96dd9c6f1fd5b3fbaa70898f09bedff903237d6d")
        );
    }

    #[test]
    fn optimism_deposited() {
        // Tx: 0x2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3
        let tx = json!({
                "OptimismDeposited": {
                    "source_hash": "0x20b925f36904e1e62099920d902925817c4357e9f674b8b14d13363196139010",
                    "from": "0x36bde71c97b33cc4729cf772ae268934f7ab70b2",
                    "to": { "Call": "0x4200000000000000000000000000000000000007" },
                    "mint": "0x030d98d59a960000",
                    "value": "0x030d98d59a960000",
                    "gas_limit": "0x077d2e",
                    "is_system_tx": false,
                    "data": "0xd764ad0b000100000000000000000000000000000000000000000000000000000000af8600000000000000000000000099c9fc46f92e8a1c0dec1b1747d010903e884be10000000000000000000000004200000000000000000000000000000000000010000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000030d4000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000000a41635f5fd000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                  }
        });
        let essence: OptimismTxEssence = serde_json::from_value(tx).unwrap();

        let transaction = OptimismTransaction {
            essence,
            signature: TxSignature::default(),
        };

        // verify the RLP roundtrip
        let decoded = Transaction::decode_bytes(alloy_rlp::encode(&transaction)).unwrap();
        assert_eq!(transaction, decoded);

        // verify that bincode serialization works
        let _: OptimismTransaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        let encoded = alloy_rlp::encode(&transaction);
        assert_eq!(encoded.len(), transaction.length());

        assert_eq!(
            transaction.hash(),
            b256!("2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3")
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            recovered,
            address!("36bde71c97b33cc4729cf772ae268934f7ab70b2")
        );
    }
}
