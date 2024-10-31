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

use alloy_primitives::{BlockNumber, B256, U256};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub trait CoreDriver: Default {
    type Block: Serialize + DeserializeOwned + 'static;
    type Header: Serialize + DeserializeOwned + 'static;
    type Receipt: Serialize + DeserializeOwned + 'static;
    type Transaction: Serialize + DeserializeOwned + 'static;

    fn parent_hash(header: &Self::Header) -> B256;
    fn header_hash(header: &Self::Header) -> B256;
    fn state_root(header: &Self::Header) -> B256;
    fn block_number(header: &Self::Header) -> BlockNumber;
    fn block_header(block: &Self::Block) -> &Self::Header;
    fn block_to_header(block: Self::Block) -> Self::Header;
    fn accumulate_difficulty(total_difficulty: U256, header: &Self::Header) -> U256;
}
