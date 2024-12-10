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

use std::sync::Arc;
use alloy::network::Network;
use alloy::primitives::{B256, U256};
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::data::StatelessClientData;

pub trait PreflightDriver<Core: CoreDriver, N: Network> {
    fn total_difficulty(header: &N::HeaderResponse) -> Option<U256>;
    fn count_transactions(block: &N::BlockResponse) -> usize;
    fn derive_transaction(transaction: N::TransactionResponse) -> Core::Transaction;
    fn derive_header(header: N::HeaderResponse) -> Core::Header;
    fn derive_block(block: N::BlockResponse, ommers: Vec<N::HeaderResponse>, chain_spec: &Arc<Core::ChainSpec>) -> Core::Block;
    fn derive_header_response(block: N::BlockResponse) -> N::HeaderResponse;
    fn header_response(block: &N::BlockResponse) -> &N::HeaderResponse;
    fn uncles(block: &N::BlockResponse) -> &Vec<B256>;
    fn derive_receipt(receipt: N::ReceiptResponse) -> Core::Receipt;
    fn derive_data(
        data: StatelessClientData<N::BlockResponse, N::HeaderResponse>,
        ommers: Vec<Vec<N::HeaderResponse>>,
        chain_spec: &Arc<Core::ChainSpec>
    ) -> StatelessClientData<Core::Block, Core::Header>;
}
