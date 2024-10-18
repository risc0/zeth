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

use alloy::consensus::{Block, BlockBody, Header, TxEnvelope, TypedTransaction};
use alloy::rpc::types::{Block as RPCBlock, Header as RPCHeader, Transaction as RPCTransaction};
use zeth_core::stateless::data::StatelessClientData;

pub trait RPCDerivableTransaction {
    fn derive(transaction: RPCTransaction) -> Self;
}

impl RPCDerivableTransaction for TypedTransaction {
    fn derive(transaction: RPCTransaction) -> Self {
        TxEnvelope::try_from(transaction).unwrap().into()
    }
}

pub trait RPCDerivableHeader {
    fn derive(header: RPCHeader) -> Self;
}

impl RPCDerivableHeader for Header {
    fn derive(header: RPCHeader) -> Self {
        Self::try_from(header).unwrap()
    }
}

pub trait RPCDerivableBlock {
    fn derive(block: RPCBlock, ommers: Vec<RPCHeader>) -> Self;
}

impl RPCDerivableBlock for Block<TypedTransaction> {
    fn derive(block: RPCBlock, ommers: Vec<RPCHeader>) -> Self {
        Self {
            header: Header::derive(block.header),
            body: BlockBody {
                transactions: block
                    .transactions
                    .into_transactions()
                    .map(TypedTransaction::derive)
                    .collect(),
                ommers: ommers
                    .into_iter()
                    .map(Header::derive)
                    .collect(),
                withdrawals: block.withdrawals,
                requests: None,
            },
        }
    }
}

pub trait RPCDerivableData {
    fn derive(data: StatelessClientData<RPCBlock, RPCHeader>, ommers: Vec<RPCHeader>) -> Self;
}

impl<B: RPCDerivableBlock, H: RPCDerivableHeader> RPCDerivableData for StatelessClientData<B, H> {
    fn derive(data: StatelessClientData<RPCBlock, RPCHeader>, ommers: Vec<RPCHeader>) -> Self {
        StatelessClientData {
            block: B::derive(data.block, ommers),
            parent_state_trie: data.parent_state_trie,
            parent_storage: data.parent_storage,
            contracts: data.contracts,
            parent_header: H::derive(data.parent_header),
            ancestor_headers: data
                .ancestor_headers
                .into_iter()
                .map(|h| H::derive(h))
                .collect(),
        }
    }
}
