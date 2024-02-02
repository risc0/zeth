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

use ethers_core::k256::sha2::{Digest, Sha256};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header,
    mmr::Hash,
    transactions::{Transaction, TxEssence},
    trie::MptNode,
    withdrawal::Withdrawal,
    Address, Bytes, RlpBytes, B256, U256,
};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct BlockBuildInput<E: TxEssence> {
    /// Block and transaction data to execute
    pub state_input: StateInput<E>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct StateInput<E: TxEssence> {
    /// Previous block header
    pub parent_header: Header,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: Address,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: U256,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: U256,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    pub transactions: Vec<Transaction<E>>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<Withdrawal>,
}

impl<E: TxEssence> StateInput<E> {
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha256::new();

        hasher.update(self.parent_header.to_rlp());
        hasher.update(self.beneficiary.0);
        hasher.update(self.gas_limit.as_le_slice());
        hasher.update(self.timestamp.as_le_slice());
        hasher.update(self.extra_data.as_ref());
        hasher.update(self.mix_hash.0);
        // todo: use precalculated trie root hashes if available
        hasher.update(self.transactions.to_rlp());
        hasher.update(self.withdrawals.to_rlp());

        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use zeth_primitives::transactions::ethereum::EthereumTxEssence;

    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = BlockBuildInput {
            state_input: StateInput::<EthereumTxEssence> {
                parent_header: Default::default(),
                beneficiary: Default::default(),
                gas_limit: Default::default(),
                timestamp: Default::default(),
                extra_data: Default::default(),
                mix_hash: Default::default(),
                transactions: vec![],
                withdrawals: vec![],
            },
            parent_state_trie: Default::default(),
            parent_storage: Default::default(),
            contracts: vec![],
            ancestor_headers: vec![],
        };
        let _: BlockBuildInput<EthereumTxEssence> =
            bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
