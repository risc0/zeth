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

use alloy_network::{Transaction, TxKind};
use alloy_primitives::{keccak256, Address, Bytes, ChainId, B256, U256};
use alloy_rlp::Encodable;
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bytes::BufMut;

/// The EIP-2718 transaction type for an Optimism deposited transaction.
pub const OPTIMISM_DEPOSITED_TX_TYPE: u8 = 0x7E;

/// Represents an Optimism depositing transaction that is a L2 transaction that was
/// derived from L1 and included in a L2 block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, RlpEncodable, RlpDecodable)]
pub struct TxOptimismDeposit {
    /// The source hash which uniquely identifies the origin of the deposit.
    pub source_hash: B256,
    /// The 160-bit address of the sender.
    pub from: Address,
    /// The 160-bit address of the intended recipient for a message call or
    /// [TxKind::Create] for contract creation.
    pub to: TxKind,
    /// The ETH value to mint on L2.
    pub mint: U256,
    /// The amount, in Wei, to be transferred to the recipient of the message call.
    pub value: U256,
    /// The maximum amount of gas allocated for the execution of the L2 transaction.
    pub gas_limit: u64,
    /// If true, the transaction does not interact with the L2 block gas pool.
    pub is_system_tx: bool,
    /// The transaction's payload, represented as a variable-length byte array.
    pub input: Bytes,
}

impl TxOptimismDeposit {
    /// Get transaction type
    pub const fn tx_type(&self) -> u8 {
        OPTIMISM_DEPOSITED_TX_TYPE
    }

    pub fn source_hash(&self) -> B256 {
        self.source_hash
    }

    pub fn from(&self) -> Address {
        self.from
    }

    pub fn hash_slow(&self) -> B256 {
        let mut buf = Vec::with_capacity(self.length() + 1);
        buf.put_u8(self.tx_type());
        self.encode(&mut buf);
        keccak256(&buf)
    }
}

impl Transaction for TxOptimismDeposit {
    fn input(&self) -> &[u8] {
        &self.input
    }

    fn to(&self) -> TxKind {
        self.to
    }

    fn value(&self) -> U256 {
        self.value
    }

    fn chain_id(&self) -> Option<ChainId> {
        None
    }

    fn nonce(&self) -> u64 {
        0
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    fn gas_price(&self) -> Option<U256> {
        None
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use hex_literal::hex;

    use super::*;

    #[test]
    fn optimism_deposited() {
        // Tx: 0x2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3
        let tx = TxOptimismDeposit {
            source_hash: b256!("20b925f36904e1e62099920d902925817c4357e9f674b8b14d13363196139010"),
            from: address!("36bde71c97b33cc4729cf772ae268934f7ab70b2"),
            to: TxKind::Call(address!("4200000000000000000000000000000000000007")),
            mint: U256::from(0x030d98d59a960000u64),
            value: U256::from(0x030d98d59a960000u64),
            gas_limit: 0x077d2e,
            is_system_tx: false,
            input: Bytes::from(hex!("d764ad0b000100000000000000000000000000000000000000000000000000000000af8600000000000000000000000099c9fc46f92e8a1c0dec1b1747d010903e884be10000000000000000000000004200000000000000000000000000000000000010000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000030d4000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000000a41635f5fd000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000")    ),
        };

        assert_eq!(
            tx.hash_slow(),
            b256!("2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3")
        );
    }
}
