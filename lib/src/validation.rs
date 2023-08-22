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

use core::fmt::Debug;

use anyhow::{bail, Result};
use hashbrown::HashMap;
use revm::primitives::{SpecId, B160 as RevmB160, B256 as RevmB256};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header, revm::to_revm_b256, transaction::Transaction, trie::MptNode,
    withdrawal::Withdrawal, BlockNumber, Bytes, B160, B256, U256,
};

use crate::consts::{ChainSpec, MAX_BLOCK_HASH_AGE, MIN_SPEC_ID};

/// External block input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Input {
    /// Previous block header
    pub parent_header: Header,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: B160,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: U256,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: U256,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    pub transactions: Vec<Transaction>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<Withdrawal>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<RevmB160, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
}

pub type StorageEntry = (MptNode, Vec<U256>);

pub fn verify_state_trie(state_trie: &MptNode, parent_state_root: &B256) -> Result<()> {
    let state_root = state_trie.hash();
    if &state_root != parent_state_root {
        bail!(
            "Invalid state trie: expected {}, got {}",
            parent_state_root,
            state_root
        );
    }

    Ok(())
}

pub fn verify_storage_trie(
    address: impl Debug,
    storage_trie: &MptNode,
    account_storage_root: &B256,
) -> Result<()> {
    let storage_root = storage_trie.hash();
    if &storage_root != account_storage_root {
        bail!(
            "Invalid storage trie for {:?}: expected {}, got {}",
            address,
            account_storage_root,
            storage_root
        );
    }

    Ok(())
}

pub fn verify_parent_chain(
    parent: &Header,
    ancestors: &[Header],
) -> Result<HashMap<u64, RevmB256>> {
    let mut block_hashes = HashMap::with_capacity(ancestors.len() + 1);
    block_hashes.insert(parent.number, to_revm_b256(parent.hash()));
    let mut prev = parent;
    for current in ancestors {
        let current_hash = current.hash();
        if prev.parent_hash != current_hash {
            bail!(
                "Invalid chain: {} is not the parent of {}",
                current.number,
                prev.number
            );
        }
        if parent.number < current.number || parent.number - current.number >= MAX_BLOCK_HASH_AGE {
            bail!(
                "Invalid chain: {} is not one of the {} most recent blocks",
                current.number,
                MAX_BLOCK_HASH_AGE,
            );
        }
        block_hashes.insert(current.number, to_revm_b256(current_hash));
        prev = current;
    }

    Ok(block_hashes)
}

pub fn compute_spec_id(chain_spec: &ChainSpec, block_number: BlockNumber) -> Result<SpecId> {
    let spec_id = chain_spec.spec_id(block_number);
    if !SpecId::enabled(spec_id, MIN_SPEC_ID) {
        bail!(
            "Invalid protocol version: expected >= {:?}, got {:?}",
            MIN_SPEC_ID,
            spec_id,
        )
    }
    Ok(spec_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = Input::default();
        let _: Input = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
