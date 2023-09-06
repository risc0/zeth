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

use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header,
    receipt::Receipt,
    transactions::{ethereum::EthereumTxEssence, Transaction},
    B256,
};

/// Input for the L2 derivation.
pub type Input = Vec<BlockInput>;

/// Input for extracting deposits.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockInput {
    /// Header of the block.
    pub block_header: Header,
    /// Transactions of the block.
    pub transactions: Vec<Transaction<EthereumTxEssence>>,
    /// Transaction receipts of the block or `None` if not required.
    pub receipts: Option<Vec<Receipt>>,
}

/// Output of extracting deposits.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Output {
    // /// L2 block hash the validation started from.
    // pub input_l2_block_hash: B256,
    // /// L1 block hash denoting the epoch of the L2 block the validation started from.
    // pub input_l1_block_hash: B256,
    /// The hash of the final block.
    pub l1_block_hash: B256,
    /// The hashes of the derived L2 blocks.
    pub l2_block_hashes: Vec<B256>,
}
