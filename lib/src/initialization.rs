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

use core::iter::once;

use anyhow::{ensure, Result};

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

    fn initialize_database(
        mut block_builder: BlockBuilder<CachedAuthDb>,
    ) -> Result<BlockBuilder<CachedAuthDb>> {
        ensure!(
            block_builder.input.parent_state_trie.hash()
                == block_builder.input.parent_header.state_root,
            "Parent state trie root mismatch!"
        );

        block_builder.db = Some(
            AuthenticatedDb::new(
                core::mem::take(&mut block_builder.input.parent_state_trie),
                core::mem::take(&mut block_builder.input.parent_storage),
                core::mem::take(&mut block_builder.input.contracts),
                block_builder
                    .input
                    .ancestor_headers
                    .iter()
                    .chain(once(&block_builder.input.parent_header))
                    .collect(),
            )?
            .into(),
        );

        Ok(block_builder)
    }
}
