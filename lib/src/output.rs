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

use serde::{Deserialize, Serialize};
use zeth_primitives::{block::Header, trie::MptNode, B256};

/// Output of block execution
#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub enum BlockBuildOutput {
    SUCCESS {
        new_block_hash: B256,
        new_block_head: Header,
        new_block_state: MptNode,
    },
    FAILURE {
        bad_input_hash: B256,
    },
}

impl BlockBuildOutput {
    pub fn success(&self) -> bool {
        match self {
            BlockBuildOutput::SUCCESS { .. } => true,
            BlockBuildOutput::FAILURE { .. } => false,
        }
    }
}
