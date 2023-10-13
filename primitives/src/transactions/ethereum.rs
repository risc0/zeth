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

use alloy_primitives::{Address, Bytes, ChainId, TxNumber, B256, U256};
use alloy_rlp::{Encodable, EMPTY_STRING_CODE};
use alloy_rlp_derive::RlpEncodable;
use anyhow::Context;
use bytes::BufMut;
use k256::{
    ecdsa::{RecoveryId, Signature as K256Signature, VerifyingKey as K256VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey as K256PublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{
    access_list::AccessList,
    keccak::keccak,
    signature::TxSignature,
    transactions::{Transaction, TxEssence},
};

/// Represents a legacy Ethereum transaction as detailed in [EIP-155](https://eips.ethereum.org/EIPS/eip-155).
///
/// The `TxEssenceLegacy` struct encapsulates the core components of a traditional
/// Ethereum transaction prior to the introduction of more recent transaction types. It
/// adheres to the specifications set out in EIP-155.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TxEssenceLegacy {
    /// The network's chain ID introduced in EIP-155 to prevent replay attacks across
    /// different chains.
    pub chain_id: Option<ChainId>,
    /// A numeric value representing the total number of transactions previously sent by
    /// the sender.
    pub nonce: TxNumber,
    /// The price, in Wei, that the sender is willing to pay per unit of gas for the
    /// transaction's execution.
    pub gas_price: U256,
    /// The maximum amount of gas allocated for the transaction's execution.
    pub gas_limit: U256,
    /// The 160-bit address of the intended recipient for a message call. For contract
    /// creation transactions, this is null.
    pub to: TransactionKind,
    /// The amount, in Wei, to be transferred to the recipient of the message call.
    pub value: U256,
    /// The transaction's payload, represented as a variable-length byte array.
    pub data: Bytes,
}

impl TxEssenceLegacy {
    /// Computes the length of the RLP-encoded payload in bytes.
    ///
    /// This method calculates the combined length of all the individual fields
    /// of the transaction when they are RLP-encoded.
    pub fn payload_length(&self) -> usize {
        self.nonce.length()
            + self.gas_price.length()
            + self.gas_limit.length()
            + self.to.length()
            + self.value.length()
            + self.data.length()
    }

    /// Encodes the transaction essence into the provided `out` buffer for the purpose of
    /// signing.
    ///
    /// The method follows the RLP encoding scheme. If a `chain_id` is present,
    /// the encoding adheres to the specifications set out in [EIP-155](https://eips.ethereum.org/EIPS/eip-155).
    pub fn signing_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        let mut payload_length = self.payload_length();
        // Append chain ID according to EIP-155 if present
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

    /// Computes the length of the RLP-encoded transaction essence in bytes, specifically
    /// for signing.
    ///
    /// This method calculates the total length of the transaction when it is RLP-encoded,
    /// including any additional bytes required for the encoding format.
    pub fn signing_length(&self) -> usize {
        let mut payload_length = self.payload_length();
        // Append chain ID according to EIP-155 if present
        if let Some(chain_id) = self.chain_id {
            payload_length += chain_id.length() + 1 + 1;
        }
        alloy_rlp::length_of_length(payload_length) + payload_length
    }
}

// Implement the Encodable trait for `TxEssenceLegacy`.
// Ensures that the `chain_id` is always ignored during the RLP encoding process.
impl Encodable for TxEssenceLegacy {
    /// Encodes the [TxEssenceLegacy] instance into the provided `out` buffer.
    ///
    /// This method follows the RLP encoding scheme, but intentionally omits the
    /// `chain_id` to ensure compatibility with legacy transactions.
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

    /// Computes the length of the RLP-encoded [TxEssenceLegacy] instance in bytes.
    ///
    /// This method calculates the total length of the transaction when it is RLP-encoded,
    /// excluding the `chain_id`.
    fn length(&self) -> usize {
        let payload_length = self.payload_length();
        alloy_rlp::length_of_length(payload_length) + payload_length
    }
}

/// Represents an Ethereum transaction with an access list, as detailed in [EIP-2930](https://eips.ethereum.org/EIPS/eip-2930).
///
/// The `TxEssenceEip2930` struct encapsulates the core components of an Ethereum
/// transaction that includes an access list. Access lists are a feature introduced in
/// EIP-2930 to specify a list of addresses and storage keys that the transaction will
/// access, allowing for more predictable gas costs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct TxEssenceEip2930 {
    /// The network's chain ID, ensuring the transaction is valid on the intended chain.
    pub chain_id: ChainId,
    /// A numeric value representing the total number of transactions previously sent by
    /// the sender.
    pub nonce: TxNumber,
    /// The price, in Wei, that the sender is willing to pay per unit of gas for the
    /// transaction's execution.
    pub gas_price: U256,
    /// The maximum amount of gas allocated for the transaction's execution.
    pub gas_limit: U256,
    /// The 160-bit address of the intended recipient for a message call. For contract
    /// creation transactions, this is null.
    pub to: TransactionKind,
    /// The amount, in Wei, to be transferred to the recipient of the message call.
    pub value: U256,
    /// The transaction's payload, represented as a variable-length byte array.
    pub data: Bytes,
    /// A list of addresses and storage keys that the transaction will access, helping in
    /// gas optimization.
    pub access_list: AccessList,
}

/// Represents an Ethereum transaction with a priority fee, as detailed in [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559).
///
/// The `TxEssenceEip1559` struct encapsulates the core components of an Ethereum
/// transaction that incorporates the priority fee mechanism introduced in EIP-1559. This
/// mechanism aims to improve the predictability of gas fees and enhance the overall user
/// experience.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct TxEssenceEip1559 {
    /// The network's chain ID, ensuring the transaction is valid on the intended chain,
    /// as introduced in EIP-155.
    pub chain_id: ChainId,
    /// A numeric value representing the total number of transactions previously sent by
    /// the sender.
    pub nonce: TxNumber,
    /// The maximum priority fee per unit of gas that the sender is willing to pay to the
    /// miner.
    pub max_priority_fee_per_gas: U256,
    /// The combined maximum fee (base + priority) per unit of gas that the sender is
    /// willing to pay for the transaction's execution.
    pub max_fee_per_gas: U256,
    /// The maximum amount of gas allocated for the transaction's execution.
    pub gas_limit: U256,
    /// The 160-bit address of the intended recipient for a message call. For contract
    /// creation transactions, this is null.
    pub to: TransactionKind,
    /// The amount, in Wei, to be transferred to the recipient of the message call.
    pub value: U256,
    /// The transaction's payload, represented as a variable-length byte array.
    pub data: Bytes,
    /// A list of addresses and storage keys that the transaction will access, aiding in
    /// gas optimization.
    pub access_list: AccessList,
}

/// Represents the type of an Ethereum transaction: either a contract creation or a call
/// to an existing contract.
///
/// This enum is used to distinguish between the two primary types of Ethereum
/// transactions. It avoids using an [Option] for this purpose because options get RLP
/// encoded into lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransactionKind {
    /// Indicates that the transaction is for creating a new contract on the Ethereum
    /// network.
    #[default]
    Create,
    /// Indicates that the transaction is a call to an existing contract, identified by
    /// its 160-bit address.
    Call(Address),
}

/// Provides a conversion from [TransactionKind] to `Option<Address>`.
///
/// This implementation allows for a straightforward extraction of the Ethereum address
/// from a [TransactionKind]. If the transaction kind is a `Call`, the address is wrapped
/// in a `Some`. If it's a `Create`, the result is `None`.
impl From<TransactionKind> for Option<Address> {
    /// Converts a [TransactionKind] into an `Option<Address>`.
    ///
    /// - If the transaction kind is `Create`, this returns `None`.
    /// - If the transaction kind is `Call`, this returns the address wrapped in a `Some`.
    fn from(value: TransactionKind) -> Self {
        match value {
            TransactionKind::Create => None,
            TransactionKind::Call(addr) => Some(addr),
        }
    }
}

/// Provides RLP encoding functionality for the [TransactionKind] enum.
///
/// This implementation ensures that each variant of the [TransactionKind] enum can be
/// RLP-encoded.
/// - The `Call` variant is encoded as the address it contains.
/// - The `Create` variant is encoded as an empty string.
impl Encodable for TransactionKind {
    /// Encodes the [TransactionKind] enum variant into the provided `out` buffer.
    ///
    /// If the transaction kind is `Call`, the Ethereum address is encoded directly.
    /// If the transaction kind is `Create`, an empty string code is added to the buffer.
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            TransactionKind::Call(addr) => addr.encode(out),
            TransactionKind::Create => out.put_u8(EMPTY_STRING_CODE),
        }
    }

    /// Computes the length of the RLP-encoded [TransactionKind] enum variant in bytes.
    ///
    /// If the transaction kind is `Call`, this returns the length of the Ethereum
    /// address. If the transaction kind is `Create`, this returns 1 (length of the
    /// empty string code).
    #[inline]
    fn length(&self) -> usize {
        match self {
            TransactionKind::Call(addr) => addr.length(),
            TransactionKind::Create => 1,
        }
    }
}

/// Represents the core essence of an Ethereum transaction, specifically the portion that
/// gets signed.
///
/// The `TxEssence` enum provides a way to handle different types of Ethereum
/// transactions, from legacy transactions to more recent types introduced by various
/// Ethereum Improvement Proposals (EIPs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EthereumTxEssence {
    /// Represents a legacy Ethereum transaction, which follows the original transaction
    /// format.
    Legacy(TxEssenceLegacy),
    /// Represents an Ethereum transaction that includes an access list, as introduced in [EIP-2930](https://eips.ethereum.org/EIPS/eip-2930).
    /// Access lists specify a list of addresses and storage keys that the transaction
    /// will access, allowing for more predictable gas costs.
    Eip2930(TxEssenceEip2930),
    /// Represents an Ethereum transaction that incorporates a priority fee mechanism, as detailed in [EIP-1559](https://eips.ethereum.org/EIPS/eip-1559).
    /// This mechanism aims to improve the predictability of gas fees and enhances the
    /// overall user experience.
    Eip1559(TxEssenceEip1559),
}

// Implement the Encodable trait for the TxEssence enum.
// Ensures that each variant of the `TxEssence` enum can be RLP-encoded.
impl Encodable for EthereumTxEssence {
    /// Encodes the [EthereumTxEssence] enum variant into the provided `out` buffer.
    ///
    /// Depending on the variant of the [EthereumTxEssence] enum, this method will
    /// delegate the encoding process to the appropriate transaction type's encoding
    /// method.
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            EthereumTxEssence::Legacy(tx) => tx.encode(out),
            EthereumTxEssence::Eip2930(tx) => tx.encode(out),
            EthereumTxEssence::Eip1559(tx) => tx.encode(out),
        }
    }

    /// Computes the length of the RLP-encoded [EthereumTxEssence] enum variant in bytes.
    ///
    /// Depending on the variant of the [EthereumTxEssence] enum, this method will
    /// delegate the length computation to the appropriate transaction type's length
    /// method.
    fn length(&self) -> usize {
        match self {
            EthereumTxEssence::Legacy(tx) => tx.length(),
            EthereumTxEssence::Eip2930(tx) => tx.length(),
            EthereumTxEssence::Eip1559(tx) => tx.length(),
        }
    }
}

impl EthereumTxEssence {
    /// Computes the signing hash for the transaction essence.
    ///
    /// This method calculates the Keccak hash of the data that needs to be signed
    /// for the transaction, ensuring the integrity and authenticity of the transaction.
    pub(crate) fn signing_hash(&self) -> B256 {
        keccak(self.signing_data()).into()
    }

    /// Retrieves the data that should be signed for the transaction essence.
    ///
    /// Depending on the variant of the [EthereumTxEssence] enum, this method prepares the
    /// appropriate data for signing. For EIP-2930 and EIP-1559 transactions, a specific
    /// prefix byte is added before the transaction data.
    fn signing_data(&self) -> Vec<u8> {
        match self {
            EthereumTxEssence::Legacy(tx) => {
                let mut buf = Vec::with_capacity(tx.signing_length());
                tx.signing_encode(&mut buf);
                buf
            }
            EthereumTxEssence::Eip2930(tx) => {
                let mut buf = Vec::with_capacity(tx.length() + 1);
                buf.push(0x01);
                tx.encode(&mut buf);
                buf
            }
            EthereumTxEssence::Eip1559(tx) => {
                let mut buf = Vec::with_capacity(tx.length() + 1);
                buf.push(0x02);
                tx.encode(&mut buf);
                buf
            }
        }
    }

    /// Determines whether the y-coordinate of the ECDSA signature's associated public key
    /// is odd.
    ///
    /// This information is derived from the `v` component of the signature and is used
    /// during public key recovery.
    fn is_y_odd(&self, signature: &TxSignature) -> Option<bool> {
        match self {
            EthereumTxEssence::Legacy(TxEssenceLegacy { chain_id: None, .. }) => {
                checked_bool(signature.v - 27)
            }
            EthereumTxEssence::Legacy(TxEssenceLegacy {
                chain_id: Some(chain_id),
                ..
            }) => checked_bool(signature.v - 35 - 2 * chain_id),
            _ => checked_bool(signature.v),
        }
    }
}

/// Converts a given value into a boolean based on its parity.
///
/// Returns:
/// - `Some(true)` if the value is 1.
/// - `Some(false)` if the value is 0.
/// - `None` otherwise.
#[inline]
pub fn checked_bool(v: u64) -> Option<bool> {
    match v {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

impl TxEssence for EthereumTxEssence {
    /// Determines the type of the transaction based on its essence.
    ///
    /// Returns a byte representing the transaction type:
    /// - `0x00` for Legacy transactions.
    /// - `0x01` for EIP-2930 transactions.
    /// - `0x02` for EIP-1559 transactions.
    fn tx_type(&self) -> u8 {
        match self {
            EthereumTxEssence::Legacy(_) => 0x00,
            EthereumTxEssence::Eip2930(_) => 0x01,
            EthereumTxEssence::Eip1559(_) => 0x02,
        }
    }
    /// Retrieves the gas limit set for the transaction.
    ///
    /// The gas limit represents the maximum amount of gas units that the transaction
    /// is allowed to consume. It ensures that transactions don't run indefinitely.
    fn gas_limit(&self) -> U256 {
        match self {
            EthereumTxEssence::Legacy(tx) => tx.gas_limit,
            EthereumTxEssence::Eip2930(tx) => tx.gas_limit,
            EthereumTxEssence::Eip1559(tx) => tx.gas_limit,
        }
    }
    /// Retrieves the recipient address of the transaction, if available.
    ///
    /// For contract creation transactions, this method returns `None` as there's no
    /// recipient address.
    fn to(&self) -> Option<Address> {
        match self {
            EthereumTxEssence::Legacy(tx) => tx.to.into(),
            EthereumTxEssence::Eip2930(tx) => tx.to.into(),
            EthereumTxEssence::Eip1559(tx) => tx.to.into(),
        }
    }
    /// Recovers the Ethereum address of the sender from the transaction's signature.
    ///
    /// This method uses the ECDSA recovery mechanism to derive the sender's public key
    /// and subsequently their Ethereum address. If the recovery is unsuccessful, an
    /// error is returned.
    fn recover_from(&self, signature: &TxSignature) -> anyhow::Result<Address> {
        let is_y_odd = self.is_y_odd(signature).context("v invalid")?;
        let signature =
            K256Signature::from_scalars(signature.r.to_be_bytes(), signature.s.to_be_bytes())
                .context("r, s invalid")?;

        let verify_key = K256VerifyingKey::recover_from_prehash(
            self.signing_hash().as_slice(),
            &signature,
            RecoveryId::new(is_y_odd, false),
        )
        .context("invalid signature")?;

        let public_key = K256PublicKey::from(&verify_key);
        let public_key = public_key.to_encoded_point(false);
        let public_key = public_key.as_bytes();
        debug_assert_eq!(public_key[0], 0x04);
        let hash = keccak(&public_key[1..]);

        Ok(Address::from_slice(&hash[12..]))
    }
    /// Computes the length of the RLP-encoded payload in bytes for the transaction
    /// essence.
    ///
    /// This method calculates the length of the transaction data when it is RLP-encoded,
    /// which is used for serialization and deserialization in the Ethereum network.
    fn payload_length(&self) -> usize {
        match self {
            EthereumTxEssence::Legacy(tx) => tx.payload_length(),
            EthereumTxEssence::Eip2930(tx) => tx._alloy_rlp_payload_length(),
            EthereumTxEssence::Eip1559(tx) => tx._alloy_rlp_payload_length(),
        }
    }

    fn encode_with_signature(&self, signature: &TxSignature, out: &mut dyn BufMut) {
        // join the essence lists and the signature list into one
        rlp_join_lists(self, signature, out);
    }

    #[inline]
    fn length(transaction: &Transaction<Self>) -> usize {
        let payload_length =
            transaction.essence.payload_length() + transaction.signature.payload_length();
        let mut length = payload_length + alloy_rlp::length_of_length(payload_length);
        if transaction.essence.tx_type() != 0 {
            length += 1;
        }
        length
    }
}

/// Joins two RLP-encoded lists into a single RLP-encoded list.
///
/// This function takes two RLP-encoded lists, decodes their headers to ensure they are
/// valid lists, and then combines their payloads into a single RLP-encoded list. The
/// resulting list is written to the provided `out` buffer.
///
/// # Arguments
///
/// * `a` - The first RLP-encoded list to be joined.
/// * `b` - The second RLP-encoded list to be joined.
/// * `out` - The buffer where the resulting RLP-encoded list will be written.
///
/// # Panics
///
/// This function will panic if either `a` or `b` are not valid RLP-encoded lists.
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
