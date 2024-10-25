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

use alloy::consensus::{Block, BlockBody, Header, TxEip4844Variant, TxEnvelope, TypedTransaction};
use alloy::network::TransactionResponse;
use alloy::rpc::types::{Block as RPCBlock, Header as RPCHeader, Transaction as RPCTransaction};
use reth_primitives::Withdrawals;
use std::iter::zip;
use zeth_core::stateless::data::StatelessClientData;

pub trait RPCDerivableTransaction {
    fn derive(transaction: RPCTransaction) -> Self;
}

impl RPCDerivableTransaction for TypedTransaction {
    fn derive(transaction: RPCTransaction) -> Self {
        TxEnvelope::try_from(transaction).unwrap().into()
    }
}

impl RPCDerivableTransaction for reth_primitives::Transaction {
    fn derive(transaction: RPCTransaction) -> Self {
        match TypedTransaction::derive(transaction) {
            TypedTransaction::Legacy(t) => Self::Legacy(t),
            TypedTransaction::Eip2930(t) => Self::Eip2930(t),
            TypedTransaction::Eip1559(t) => Self::Eip1559(t),
            TypedTransaction::Eip4844(t) => match t {
                TxEip4844Variant::TxEip4844(t) => Self::Eip4844(t),
                TxEip4844Variant::TxEip4844WithSidecar(t) => Self::Eip4844(t.tx),
            },
            TypedTransaction::Eip7702(t) => Self::Eip7702(t),
        }
    }
}

impl RPCDerivableTransaction for reth_primitives::TransactionSigned {
    fn derive(transaction: RPCTransaction) -> Self {
        let signature = transaction.signature().unwrap();
        Self {
            hash: transaction.hash,
            signature: signature.try_into().unwrap(),
            transaction: reth_primitives::Transaction::derive(transaction),
        }
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
                ommers: ommers.into_iter().map(Header::derive).collect(),
                withdrawals: block.withdrawals,
                requests: None,
            },
        }
    }
}

impl RPCDerivableBlock for reth_primitives::Block {
    fn derive(block: RPCBlock, ommers: Vec<RPCHeader>) -> Self {
        Self {
            header: Header::derive(block.header),
            body: reth_primitives::BlockBody {
                transactions: block
                    .transactions
                    .into_transactions()
                    .map(reth_primitives::TransactionSigned::derive)
                    .collect(),
                ommers: ommers.into_iter().map(Header::derive).collect(),
                withdrawals: block.withdrawals.map(|w| Withdrawals::new(w)),
                requests: None,
            },
        }
    }
}

pub trait RPCDerivableData {
    fn derive(data: StatelessClientData<RPCBlock, RPCHeader>, ommers: Vec<Vec<RPCHeader>>) -> Self;
}

impl<B: RPCDerivableBlock, H: RPCDerivableHeader> RPCDerivableData for StatelessClientData<B, H> {
    fn derive(data: StatelessClientData<RPCBlock, RPCHeader>, ommers: Vec<Vec<RPCHeader>>) -> Self {
        StatelessClientData {
            blocks: zip(data.blocks.into_iter(), ommers.into_iter())
                .map(|(block, ommers)| B::derive(block, ommers))
                .collect(),
            state_trie: data.state_trie,
            storage_tries: data.storage_tries,
            contracts: data.contracts,
            parent_header: H::derive(data.parent_header),
            ancestor_headers: data
                .ancestor_headers
                .into_iter()
                .map(|h| H::derive(h))
                .collect(),
            total_difficulty: data.total_difficulty,
        }
    }
}
