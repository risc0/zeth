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

use alloy::network::ReceiptResponse;
use alloy::network::{Ethereum, Network};
use alloy::primitives::{B256, U256};
use alloy::rpc::types::serde_helpers::WithOtherFields;
use alloy::signers::k256::ecdsa::VerifyingKey;
use reth_chainspec::ChainSpec;
use reth_primitives::{Block, BlockBody, Header, Log, Receipt, TransactionSigned, Withdrawals};
use std::iter::zip;
use std::sync::Arc;
use zeth_core::db::memory::MemoryDB;
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core_ethereum::RethStatelessClient;
use zeth_core_ethereum::RethValidationStrategy;
use zeth_core_ethereum::{RethCoreDriver, RethExecutionStrategy};
use zeth_preflight::client::PreflightClient;
use zeth_preflight::driver::PreflightDriver;
use zeth_preflight::BlockBuilder;
use zeth_preflight::provider::types::NilNetwork;

#[derive(Clone)]
pub struct RethBlockBuilder {
    pub chain_spec: Arc<ChainSpec>,
}

impl BlockBuilder<NilNetwork, MemoryDB, RethCoreDriver, RethPreflightDriver> for RethBlockBuilder {
    type PreflightClient = RethPreflightClient;
    type StatelessClient = RethStatelessClient;
}

#[derive(Clone)]
pub struct RethPreflightClient;

impl PreflightClient<NilNetwork, RethCoreDriver, RethPreflightDriver> for RethPreflightClient {
    type Validation = RethValidationStrategy;
    type Execution = RethExecutionStrategy;
}

#[derive(Clone)]
pub struct RethPreflightDriver;

impl PreflightDriver<RethCoreDriver, NilNetwork> for RethPreflightDriver {
    fn total_difficulty(header: &<NilNetwork as Network>::HeaderResponse) -> Option<U256> {
        None//header.total_difficulty
    }

    fn count_transactions(block: &<NilNetwork as Network>::BlockResponse) -> usize {
        block.transactions.len()
    }

    fn derive_transaction(
        transaction: <NilNetwork as Network>::TransactionResponse,
    ) -> <RethCoreDriver as CoreDriver>::Transaction {
        todo!()//TransactionSigned::try_from(WithOtherFields::new(transaction)).unwrap()
    }

    fn derive_header(header: <NilNetwork as Network>::HeaderResponse) -> Header {
        todo!()//Header::try_from(header).unwrap()
    }

    fn derive_block(
        block: <NilNetwork as Network>::BlockResponse,
        ommers: Vec<<NilNetwork as Network>::HeaderResponse>,
    ) -> Block {
        Block {
            header: Self::derive_header(block.header),
            body: BlockBody {
                transactions: block
                    .transactions
                    .into_transactions()
                    .map(Self::derive_transaction)
                    .collect(),
                ommers: ommers.into_iter().map(Self::derive_header).collect(),
                withdrawals: None,//block.withdrawals.map(Withdrawals::new),
                requests: None,
            },
        }
    }

    fn derive_header_response(
        block: <NilNetwork as Network>::BlockResponse,
    ) -> <NilNetwork as Network>::HeaderResponse {
        block.header
    }

    fn header_response(
        block: &<NilNetwork as Network>::BlockResponse,
    ) -> &<NilNetwork as Network>::HeaderResponse {
        &block.header
    }

    fn uncles(block: &<NilNetwork as Network>::BlockResponse) -> &Vec<B256> {
        todo!()//&block.uncles
    }

    fn derive_receipt(
        receipt: <NilNetwork as Network>::ReceiptResponse,
    ) -> <RethCoreDriver as CoreDriver>::Receipt {
        let inner = receipt.inner.as_receipt().unwrap();
        let logs = inner
            .logs
            .iter()
            .map(|log| Log {
                address: log.address(),
                data: log.data().clone(),
            })
            .collect();
        Receipt {
            tx_type: receipt.transaction_type().into(),
            success: receipt.status(),
            cumulative_gas_used: receipt.cumulative_gas_used() as u64,
            logs,
        }
    }

    fn derive_data(
        data: StatelessClientData<
            <NilNetwork as Network>::BlockResponse,
            <NilNetwork as Network>::HeaderResponse,
        >,
        ommers: Vec<Vec<<NilNetwork as Network>::HeaderResponse>>,
    ) -> StatelessClientData<Block, Header> {
        let blocks: Vec<_> = zip(data.blocks, ommers)
            .map(|(block, ommers)| Self::derive_block(block, ommers))
            .collect();
        let signers = blocks.iter().map(Self::recover_signers).collect();
        StatelessClientData {
            chain: data.chain,
            blocks,
            signers,
            state_trie: data.state_trie,
            storage_tries: data.storage_tries,
            contracts: data.contracts,
            parent_header: Self::derive_header(data.parent_header),
            ancestor_headers: data
                .ancestor_headers
                .into_iter()
                .map(Self::derive_header)
                .collect(),
            total_difficulty: data.total_difficulty,
        }
    }

    fn recover_signers(block: &Block) -> Vec<VerifyingKey> {
        block
            .body
            .transactions()
            .map(|tx| {
                tx.signature()
                    .recover_from_prehash(&tx.signature_hash())
                    .unwrap()
            })
            .collect()
    }
}
