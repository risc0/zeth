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

use anyhow::Result;
use revm::{Database, DatabaseCommit};

use super::BlockBuilder;

pub(super) mod taiko;

pub trait TxExecStrategy {
    fn execute_transactions<D>(block_builder: BlockBuilder<D>) -> Result<BlockBuilder<D>>
    where
        D: Database + DatabaseCommit,
        <D as Database>::Error: Debug;
}
