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

use alloy_primitives::{Address, B256, U256};
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use reth_revm::DatabaseRef;
use reth_revm::db::CacheDB;
use reth_storage_errors::db::DatabaseError;

pub type MemoryDB = CacheDB<UnreachableDB>;

#[derive(Clone, Copy, Default)]
pub struct UnreachableDB;

impl DatabaseRef for UnreachableDB {
    type Error = DatabaseError;

    fn basic_ref(&self, _: Address) -> Result<Option<AccountInfo>, Self::Error> {
        unreachable!()
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, Self::Error> {
        unreachable!()
    }

    fn storage_ref(&self, _: Address, _: U256) -> Result<U256, Self::Error> {
        unreachable!()
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        unreachable!()
    }
}