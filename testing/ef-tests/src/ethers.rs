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

use ethers_core::types::{
    Block, Bloom, Bytes, EIP1186ProofResponse, StorageProof, Transaction, H256, U256,
};
use revm::primitives::B160 as RevmB160;
use zeth_primitives::U256 as LibU256;

use super::*;

/// Provider that always returns the default if not contained in the state.
pub struct TestProvider {
    pub state: TestState,
    pub header: Header,
}

impl Provider for TestProvider {
    fn save(&self) -> Result<(), anyhow::Error> {
        unimplemented!()
    }

    fn get_full_block(&mut self, _: &BlockQuery) -> Result<Block<Transaction>, anyhow::Error> {
        unimplemented!()
    }

    fn get_partial_block(&mut self, query: &BlockQuery) -> Result<Block<H256>, anyhow::Error> {
        if query.block_no != self.header.number {
            bail!("block {} not found", query.block_no);
        }

        Ok(Block::<H256> {
            parent_hash: self.header.parent_hash.0.into(),
            uncles_hash: self.header.ommers_hash.0.into(),
            author: Some(self.header.beneficiary.0 .0.into()),
            state_root: self.header.state_root.0.into(),
            transactions_root: self.header.transactions_root.0.into(),
            receipts_root: self.header.receipts_root.0.into(),
            logs_bloom: Some(Bloom::from_slice(self.header.logs_bloom.as_slice())),
            difficulty: self.header.difficulty.to_be_bytes().into(),
            number: Some(self.header.number.into()),
            gas_limit: self.header.gas_limit.to_be_bytes().into(),
            gas_used: self.header.gas_used.to_be_bytes().into(),
            timestamp: self.header.timestamp.to_be_bytes().into(),
            extra_data: self.header.extra_data.0.clone().into(),
            mix_hash: Some(self.header.mix_hash.0.into()),
            nonce: Some(self.header.nonce.0.into()),
            base_fee_per_gas: Some(self.header.base_fee_per_gas.to_be_bytes().into()),
            withdrawals_root: self.header.withdrawals_root.map(|r| r.0.into()),
            hash: Some(self.header.hash().0.into()),
            ..Default::default()
        })
    }

    fn get_proof(&mut self, query: &ProofQuery) -> Result<EIP1186ProofResponse, anyhow::Error> {
        assert_eq!(query.block_no, self.header.number);

        let indices = query
            .indices
            .iter()
            .map(|idx| LibU256::from_be_bytes(idx.0));
        get_proof(from_ethers_h160(query.address), indices, &self.state)
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> Result<U256, anyhow::Error> {
        assert_eq!(query.block_no, self.header.number);

        let nonce: u64 = self
            .state
            .0
            .get(&from_ethers_h160(query.address))
            .map(|account| account.nonce.try_into().unwrap())
            .unwrap_or_default();
        Ok(U256::from(nonce))
    }

    fn get_balance(&mut self, query: &AccountQuery) -> Result<U256, anyhow::Error> {
        assert_eq!(query.block_no, self.header.number);

        let balance = self
            .state
            .0
            .get(&from_ethers_h160(query.address))
            .map(|account| account.balance)
            .unwrap_or_default();
        Ok(balance.to_be_bytes().into())
    }

    fn get_code(&mut self, query: &AccountQuery) -> Result<Bytes, anyhow::Error> {
        assert_eq!(query.block_no, self.header.number);

        let code = self
            .state
            .0
            .get(&from_ethers_h160(query.address))
            .map(|account| account.code.clone())
            .unwrap_or_default();
        Ok(code.0.into())
    }

    fn get_storage(&mut self, query: &StorageQuery) -> Result<H256, anyhow::Error> {
        assert_eq!(query.block_no, self.header.number);

        match self.state.0.get(&from_ethers_h160(query.address)) {
            Some(account) => {
                let value = account
                    .storage
                    .get(&LibU256::from_be_bytes(query.index.0))
                    .cloned()
                    .unwrap_or_default();
                Ok(value.to_be_bytes().into())
            }
            None => Ok(H256::zero()),
        }
    }
}

fn build_tries(state: &TestState) -> (MptNode, HashMap<Address, MptNode>) {
    let mut state_trie = MptNode::default();
    let mut storage_tries = HashMap::new();
    for (address, account) in &state.0 {
        let mut storage_trie = MptNode::default();
        for (slot, value) in &account.storage {
            if *value != LibU256::ZERO {
                storage_trie
                    .insert_rlp(&keccak(slot.to_be_bytes::<32>()), *value)
                    .unwrap();
            }
        }

        state_trie
            .insert_rlp(
                &keccak(address),
                StateAccount {
                    nonce: account.nonce.try_into().unwrap(),
                    balance: account.balance,
                    storage_root: storage_trie.hash(),
                    code_hash: keccak(account.code.clone()).into(),
                },
            )
            .unwrap();
        storage_tries.insert(*address, storage_trie);
    }

    (state_trie, storage_tries)
}

fn get_proof(
    address: Address,
    indices: impl IntoIterator<Item = LibU256>,
    state: &TestState,
) -> Result<EIP1186ProofResponse, anyhow::Error> {
    let account = state.0.get(&address).cloned().unwrap_or_default();
    let (state_trie, mut storage_tries) = build_tries(state);
    let storage_trie = storage_tries.remove(&address).unwrap_or_default();

    let account_proof = mpt_proof(&state_trie, keccak(address))?
        .into_iter()
        .map(|p| p.into())
        .collect();
    let mut storage_proof = vec![];
    for index in indices {
        let proof = StorageProof {
            key: index.to_be_bytes().into(),
            proof: mpt_proof(&storage_trie, keccak(index.to_be_bytes::<32>()))?
                .into_iter()
                .map(|p| p.into())
                .collect(),
            value: account
                .storage
                .get(&index)
                .cloned()
                .unwrap_or_default()
                .to_be_bytes()
                .into(),
        };
        storage_proof.push(proof);
    }

    Ok(EIP1186ProofResponse {
        address: address.0 .0.into(),
        balance: account.balance.to_be_bytes().into(),
        code_hash: keccak(account.code).into(),
        nonce: account.nonce.to_be_bytes().into(),
        storage_hash: storage_trie.hash().0.into(),
        account_proof,
        storage_proof,
    })
}

/// Get EIP-1186 proofs for a set of addresses and storage keys.
pub fn get_state_update_proofs(
    provider: &ProviderDb,
    storage_keys: HashMap<RevmB160, Vec<LibU256>>,
) -> Result<HashMap<RevmB160, EIP1186ProofResponse>, anyhow::Error> {
    let state = provider.into();

    let mut result = HashMap::new();
    for (address, indices) in storage_keys {
        result.insert(
            address,
            get_proof(from_revm_b160(address), indices, &state)?,
        );
    }
    Ok(result)
}
