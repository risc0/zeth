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

use alloy_rlp_derive::RlpEncodable;
use ethers_core::k256::sha2::{Digest, Sha256};
use revm::primitives::HashMap;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zeth_primitives::{
    mmr::Hash, serde_with::RlpBytes, transactions::TxEnvelope, trie::MptNode,
    withdrawal::Withdrawal, Address, Bytes, Header, B256, U256,
};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[serde_as]
#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct BlockBuildInput {
    /// Block and transaction data to execute
    pub state_input: StateInput,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    #[serde_as(as = "Vec<RlpBytes>")]
    pub ancestor_headers: Vec<Header>,
}

#[serde_as]
#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize, RlpEncodable)]
#[rlp(trailing)]
pub struct StateInput {
    /// Previous block header
    #[serde_as(as = "RlpBytes")]
    pub parent_header: Header,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: Address,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: u64,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: u64,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    pub transactions: Vec<TxEnvelope>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<Withdrawal>,
    pub parent_beacon_block_root: Option<B256>,
}

impl StateInput {
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&alloy_rlp::encode(self));
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input: BlockBuildInput = Default::default();
        let _: BlockBuildInput =
            bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
