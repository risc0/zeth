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
use core::{option::Option, result::Result::*};

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_rlp::Encodable;
use alloy_rlp_derive::RlpEncodable;
use bytes::BufMut;
use serde::{Deserialize, Serialize};

use crate::{
    signature::TxSignature,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        Transaction, TxEssence,
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable)]
pub struct TxEssenceOptimismDeposited {
    /// The source hash which uniquely identifies the origin of the deposit
    pub source_hash: B256,
    /// The 160-bit address of the sender.
    pub from: Address,
    /// The 160-bit address of the message call's recipient or, for a contract creation
    /// transaction, ∅.
    pub to: TransactionKind,
    /// The ETH value to mint on L2
    pub mint: U256,
    /// A scalar value equal to the number of Wei to be transferred to the message call's
    /// recipient.
    pub value: U256,
    /// A scalar value equal to the maximum amount of gas that should be used in executing
    /// this transaction.
    pub gas_limit: U256,
    /// If true, the transaction does not interact with the L2 block gas pool.
    /// Note: boolean is disabled (enforced to be false) starting from the Regolith
    /// upgrade.
    pub is_system_tx: bool,
    /// An unlimited size byte array specifying the transaction data.
    pub data: Bytes,
}

impl TxEssenceOptimismDeposited {
    pub fn payload_length(&self) -> usize {
        self.source_hash.length()
            + self.from.length()
            + self.to.length()
            + self.mint.length()
            + self.value.length()
            + self.gas_limit.length()
            + self.is_system_tx.length()
            + self.data.length()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimismTxEssence {
    Ethereum(EthereumTxEssence),
    OptimismDeposited(TxEssenceOptimismDeposited),
}

impl Encodable for OptimismTxEssence {
    fn encode(&self, out: &mut dyn BufMut) {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.encode(out),
            OptimismTxEssence::OptimismDeposited(op) => op.encode(out),
        }
    }

    fn length(&self) -> usize {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.length(),
            OptimismTxEssence::OptimismDeposited(op) => op.length(),
        }
    }
}

impl TxEssence for OptimismTxEssence {
    fn tx_type(&self) -> u8 {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.tx_type(),
            OptimismTxEssence::OptimismDeposited(_) => 0x7E,
        }
    }

    fn gas_limit(&self) -> U256 {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.gas_limit(),
            OptimismTxEssence::OptimismDeposited(op) => op.gas_limit,
        }
    }

    fn to(&self) -> Option<Address> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.to(),
            OptimismTxEssence::OptimismDeposited(op) => op.to.into(),
        }
    }

    fn recover_from(&self, signature: &TxSignature) -> anyhow::Result<Address> {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.recover_from(signature),
            OptimismTxEssence::OptimismDeposited(op) => Ok(op.from),
        }
    }

    fn payload_length(&self) -> usize {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.payload_length(),
            OptimismTxEssence::OptimismDeposited(op) => op.payload_length(),
        }
    }

    fn encode_with_signature(&self, signature: &TxSignature, out: &mut dyn BufMut) {
        match self {
            OptimismTxEssence::Ethereum(eth) => eth.encode_with_signature(signature, out),
            OptimismTxEssence::OptimismDeposited(op) => op.encode(out),
        }
    }

    #[inline]
    fn length(transaction: &Transaction<Self>) -> usize {
        let payload_length = match &transaction.essence {
            OptimismTxEssence::Ethereum(eth) => {
                eth.payload_length() + transaction.signature.payload_length()
            }
            OptimismTxEssence::OptimismDeposited(op) => op.payload_length(),
        };
        let mut length = payload_length + alloy_rlp::length_of_length(payload_length);
        if transaction.essence.tx_type() != 0 {
            length += 1;
        }
        length
    }
}
