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

use alloy::network::{Network, ReceiptResponse};
use alloy::primitives::{Log, B256, U256};
use alloy::rpc::types::serde_helpers::WithOtherFields;
use op_alloy_consensus::OpTxType;
use op_alloy_network::Optimism;
use reth_optimism_chainspec::{OpChainSpec, OP_MAINNET};
use reth_primitives::{Block, BlockBody, Header, Receipt, TransactionSigned, TxType, Withdrawals};
use std::iter::zip;
use std::sync::Arc;
use zeth_core::db::MemoryDB;
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core_optimism::{
    OpRethCoreDriver, OpRethExecutionStrategy, OpRethStatelessClient, OpRethValidationStrategy,
};
use zeth_preflight::client::PreflightClient;
use zeth_preflight::driver::PreflightDriver;
use zeth_preflight::BlockBuilder;

#[derive(Clone)]
pub struct OpRethBlockBuilder;

impl BlockBuilder<OpChainSpec, Optimism, MemoryDB, OpRethCoreDriver, OpRethPreflightDriver>
    for OpRethBlockBuilder
{
    type PreflightClient = OpRethPreflightClient;
    type StatelessClient = OpRethStatelessClient;

    fn chain_spec() -> Arc<OpChainSpec> {
        OP_MAINNET.clone()
    }
}

#[derive(Clone)]
pub struct OpRethPreflightClient;

impl PreflightClient<OpChainSpec, Optimism, OpRethCoreDriver, OpRethPreflightDriver>
    for OpRethPreflightClient
{
    type Validation = OpRethValidationStrategy;
    type Execution = OpRethExecutionStrategy;
}

#[derive(Clone)]
pub struct OpRethPreflightDriver;

impl PreflightDriver<OpRethCoreDriver, Optimism> for OpRethPreflightDriver {
    fn total_difficulty(header: &<Optimism as Network>::HeaderResponse) -> Option<U256> {
        header.total_difficulty
    }

    fn count_transactions(block: &<Optimism as Network>::BlockResponse) -> usize {
        block.transactions.len()
    }

    fn derive_transaction(
        transaction: <Optimism as Network>::TransactionResponse,
    ) -> <OpRethCoreDriver as CoreDriver>::Transaction {
        let encoded = serde_json::to_vec(&transaction).unwrap();
        let decoded: WithOtherFields<_> = serde_json::from_slice(&encoded).unwrap();
        TransactionSigned::try_from(decoded).unwrap()
    }

    fn derive_header(
        header: <Optimism as Network>::HeaderResponse,
    ) -> <OpRethCoreDriver as CoreDriver>::Header {
        Header::try_from(header).unwrap()
    }

    fn derive_block(
        block: <Optimism as Network>::BlockResponse,
        ommers: Vec<<Optimism as Network>::HeaderResponse>,
    ) -> <OpRethCoreDriver as CoreDriver>::Block {
        Block {
            header: Self::derive_header(block.header),
            body: BlockBody {
                transactions: block
                    .transactions
                    .into_transactions()
                    .map(Self::derive_transaction)
                    .collect(),
                ommers: ommers.into_iter().map(Self::derive_header).collect(),
                withdrawals: block.withdrawals.map(Withdrawals::new),
                requests: None,
            },
        }
    }

    fn derive_header_response(
        block: <Optimism as Network>::BlockResponse,
    ) -> <Optimism as Network>::HeaderResponse {
        block.header
    }

    fn header_response(
        block: &<Optimism as Network>::BlockResponse,
    ) -> &<Optimism as Network>::HeaderResponse {
        &block.header
    }

    fn uncles(block: &<Optimism as Network>::BlockResponse) -> &Vec<B256> {
        &block.uncles
    }

    fn derive_receipt(
        receipt: <Optimism as Network>::ReceiptResponse,
    ) -> <OpRethCoreDriver as CoreDriver>::Receipt {
        let inner = receipt.inner.inner.as_receipt().unwrap();
        let logs = inner
            .logs
            .iter()
            .map(|log| Log {
                address: log.address(),
                data: log.data().clone(),
            })
            .collect();
        let tx_type = match receipt.inner.inner.tx_type() {
            OpTxType::Legacy => TxType::Legacy,
            OpTxType::Eip2930 => TxType::Eip2930,
            OpTxType::Eip1559 => TxType::Eip1559,
            OpTxType::Eip7702 => TxType::Eip7702,
            OpTxType::Deposit => TxType::Deposit,
        };
        Receipt {
            tx_type,
            success: receipt.status(),
            cumulative_gas_used: receipt.cumulative_gas_used() as u64,
            logs,
            deposit_nonce: receipt.inner.inner.deposit_nonce(),
            deposit_receipt_version: receipt.inner.inner.deposit_receipt_version(),
        }
    }

    fn derive_data(
        data: StatelessClientData<
            <Optimism as Network>::BlockResponse,
            <Optimism as Network>::HeaderResponse,
        >,
        ommers: Vec<Vec<<Optimism as Network>::HeaderResponse>>,
    ) -> StatelessClientData<
        <OpRethCoreDriver as CoreDriver>::Block,
        <OpRethCoreDriver as CoreDriver>::Header,
    > {
        StatelessClientData {
            blocks: zip(data.blocks, ommers)
                .map(|(block, ommers)| Self::derive_block(block, ommers))
                .collect(),
            state_trie: data.state_trie,
            storage_tries: data.storage_tries,
            contracts: data.contracts,
            parent_header: Self::derive_header(data.parent_header),
            ancestor_headers: data
                .ancestor_headers
                .into_iter()
                .map(|h| Self::derive_header(h))
                .collect(),
            total_difficulty: data.total_difficulty,
        }
    }
}
