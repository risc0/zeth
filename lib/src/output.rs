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
        hash: B256,
        head: Header,
        state: MptNode,
        state_input_hash: B256,
    },
    FAILURE {
        state_input_hash: B256,
    },
}

impl BlockBuildOutput {
    /// Returns true iff of type [`BlockBuildOutput::SUCCESS`]
    pub fn success(&self) -> bool {
        match self {
            BlockBuildOutput::SUCCESS { .. } => true,
            BlockBuildOutput::FAILURE { .. } => false,
        }
    }

    pub fn state_input_hash(&self) -> &B256 {
        match self {
            BlockBuildOutput::SUCCESS {
                state_input_hash, ..
            }
            | BlockBuildOutput::FAILURE {
                state_input_hash, ..
            } => state_input_hash,
        }
    }

    /// Replaces the `state` [`MptNode`] with its root hash, returning the original state.
    pub fn replace_state_with_hash(&mut self) -> Option<MptNode> {
        if let BlockBuildOutput::SUCCESS {
            head: new_block_head,
            state: new_block_state,
            ..
        } = self
        {
            Some(core::mem::replace(
                new_block_state,
                new_block_head.state_root.into(),
            ))
        } else {
            None
        }
    }

    /// Returns a new instance where `state` [`MptNode`] is replaced with its root hash
    pub fn with_state_hashed(mut self) -> Self {
        self.replace_state_with_hash();
        self
    }
}
