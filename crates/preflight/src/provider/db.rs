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

use crate::provider::{AccountQuery, BlockQuery, Provider, StorageQuery};
use alloy::primitives::map::HashMap;
use alloy::primitives::{Address, B256, U256};
use reth_revm::primitives::{Account, AccountInfo, Bytecode};
use reth_revm::{Database, DatabaseCommit};

pub struct ProviderDb {
    pub provider: Box<dyn Provider>,
    pub block_no: u64,
}

impl ProviderDb {
    pub fn new(provider: Box<dyn Provider>, block_no: u64) -> Self {
        ProviderDb { provider, block_no }
    }

    pub fn save_provider(&self) -> anyhow::Result<()> {
        self.provider.save()
    }

    // fn get_proofs(
    //     &mut self,
    //     block_no: u64,
    //     storage_keys: HashMap<Address, Vec<U256>>,
    // ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
    //     let mut out = HashMap::new();
    //
    //     for (address, indices) in storage_keys {
    //         let proof = {
    //             let address: Address = address.into_array().into();
    //             let indices: BTreeSet<B256> = indices
    //                 .into_iter()
    //                 .map(|x| x.to_be_bytes().into())
    //                 .collect();
    //             self.provider.get_proof(&ProofQuery {
    //                 block_no,
    //                 address,
    //                 indices,
    //             })?
    //         };
    //         out.insert(address, proof);
    //     }
    //
    //     Ok(out)
    // }
    //
    // pub fn get_initial_proofs(
    //     &mut self,
    // ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
    //     self.get_proofs(self.block_no, self.initial_db.storage_keys())
    // }
    //
    // pub fn get_latest_proofs(
    //     &mut self,
    // ) -> Result<HashMap<Address, EIP1186AccountProofResponse>, anyhow::Error> {
    //     let mut storage_keys = self.initial_db.storage_keys();
    //
    //     for (address, mut indices) in self.latest_db.storage_keys() {
    //         match storage_keys.get_mut(&address) {
    //             Some(initial_indices) => initial_indices.append(&mut indices),
    //             None => {
    //                 storage_keys.insert(address, indices);
    //             }
    //         }
    //     }
    //
    //     self.get_proofs(self.block_no + 1, storage_keys)
    // }
    //
    // pub fn get_ancestor_headers(&mut self) -> Result<Vec<Header>, anyhow::Error> {
    //     let earliest_block = self
    //         .initial_db
    //         .block_hashes
    //         .keys()
    //         .min()
    //         .to::<u64>()
    //         .unwrap_or(self.block_no);
    //     let headers = (earliest_block..self.block_no)
    //         .rev()
    //         .map(|block_no| {
    //             self.provider
    //                 .get_full_block(&BlockQuery { block_no })
    //                 .expect("Failed to retrieve ancestor block")
    //                 .try_into()
    //                 .expect("Failed to convert ethers block to zeth block")
    //         })
    //         .collect();
    //     Ok(headers)
    // }
}

impl Database for ProviderDb {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let query = AccountQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
        };
        let nonce = self.provider.get_transaction_count(&query)?;
        let balance = self.provider.get_balance(&query)?;
        let code = self.provider.get_code(&query)?;
        let bytecode = Bytecode::new_raw(code);
        Ok(Some(AccountInfo::new(
            balance,
            nonce.to(),
            bytecode.hash_slow(),
            bytecode,
        )))
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // not needed because we already load code with basic info
        unreachable!()
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let bytes = index.to_be_bytes::<32>();
        let index = U256::from_be_bytes(bytes);

        self.provider.get_storage(&StorageQuery {
            block_no: self.block_no,
            address: address.into_array().into(),
            index,
        })
    }

    fn block_hash(&mut self, block_no: u64) -> Result<B256, Self::Error> {
        Ok(self
            .provider
            .get_full_block(&BlockQuery { block_no })?
            .header
            .hash)
    }
}

impl DatabaseCommit for ProviderDb {
    fn commit(&mut self, _changes: HashMap<Address, Account>) {}
}
