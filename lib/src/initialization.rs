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

use core::{iter::once, mem::take};

use anyhow::{bail, Result};
use hashbrown::HashMap;
use revm::{
    db::CacheDB,
    primitives::{Bytecode, B256, KECCAK_EMPTY},
};
use ruint::aliases::U256;
use zeth_primitives::{keccak::keccak, revm::to_revm_b256};

use crate::{
    auth_db::{AuthenticatedDb, CachedAuthDb},
    block_builder::BlockBuilder,
};

pub trait DbInitStrategy {
    type Db;

    fn initialize_database(block_builder: BlockBuilder<Self::Db>)
        -> Result<BlockBuilder<Self::Db>>;
}

pub struct CachedAuthDbFromInputStrategy {}

impl DbInitStrategy for CachedAuthDbFromInputStrategy {
    type Db = CachedAuthDb;

    #[inline(always)]
    fn initialize_database(
        mut block_builder: BlockBuilder<CachedAuthDb>,
    ) -> Result<BlockBuilder<CachedAuthDb>> {
        // authenticate state root
        if block_builder.input.parent_state_trie.hash()
            != block_builder.input.parent_header.state_root
        {
            bail!("Parent state trie root mismatch!");
        }
        // authenticate historical block hashes
        let mut block_hashes: HashMap<U256, B256> =
            HashMap::with_capacity(block_builder.input.ancestor_headers.len() + 1);
        block_builder
            .input
            .ancestor_headers
            .iter()
            .rev()
            .chain(once(&block_builder.input.parent_header))
            .fold(Ok(None), |previous, current| {
                if let Ok(Some(parent_hash)) = previous {
                    if parent_hash != current.parent_hash {
                        bail!("Invalid historical block sequence!")
                    }
                }
                let current_block_hash = current.hash();
                block_hashes.insert(current.number.try_into()?, to_revm_b256(current_block_hash));
                Ok(Some(current_block_hash))
            })?;
        // authenticate bytecode
        let mut code_map = HashMap::with_capacity(block_builder.input.contracts.len() + 2);
        code_map.insert(KECCAK_EMPTY, Bytecode::new());
        code_map.insert(B256::zero(), Bytecode::new());
        code_map.extend(
            take(&mut block_builder.input.contracts)
                .into_iter()
                .map(|bytes| unsafe {
                    let hash = keccak(&bytes).into();
                    (hash, Bytecode::new_raw_with_hash(bytes.0, hash))
                }),
        );
        // Set database
        block_builder.db = Some(CacheDB {
            accounts: HashMap::with_capacity(block_builder.input.parent_storage.len()),
            contracts: code_map,
            logs: Default::default(),
            block_hashes,
            db: AuthenticatedDb {
                state_trie: take(&mut block_builder.input.parent_state_trie),
                storage_tries: take(&mut block_builder.input.parent_storage),
            },
        });
        // Give back builder
        Ok(block_builder)
    }
}
