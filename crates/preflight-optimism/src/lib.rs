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

use alloy::consensus::TxReceipt;
use alloy::network::Network;
use alloy::primitives::{Log, B256, U256};
use alloy::signers::k256::ecdsa::VerifyingKey;
use op_alloy_consensus::{OpBlock, OpDepositReceipt, OpTxType};
use op_alloy_network::Optimism;
use reth_codecs::alloy::transaction::Envelope;
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_primitives::OpReceipt;
use reth_primitives::{Block, BlockBody, Header};
use std::iter::zip;
use std::sync::Arc;
use zeth_core::db::memory::MemoryDB;
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core_optimism::{
    op_signature_hash, OpRethCoreDriver, OpRethExecutionStrategy, OpRethStatelessClient,
    OpRethValidationStrategy,
};
use zeth_preflight::client::PreflightClient;
use zeth_preflight::driver::PreflightDriver;
use zeth_preflight::BlockBuilder;

#[derive(Clone)]
pub struct OpRethBlockBuilder {
    pub chain_spec: Arc<OpChainSpec>,
}

impl BlockBuilder<Optimism, MemoryDB, OpRethCoreDriver, OpRethPreflightDriver>
    for OpRethBlockBuilder
{
    type PreflightClient = OpRethPreflightClient;
    type StatelessClient = OpRethStatelessClient;
}

#[derive(Clone)]
pub struct OpRethPreflightClient;

impl PreflightClient<Optimism, OpRethCoreDriver, OpRethPreflightDriver> for OpRethPreflightClient {
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
        transaction.inner.into_inner()
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
                withdrawals: block.withdrawals,
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
        // Move logs
        let logs = receipt
            .inner
            .inner
            .logs()
            .iter()
            .map(|log| Log {
                address: log.address(),
                data: log.data().clone(),
            })
            .collect();

        if let Some(deposit_receipt) = receipt.inner.inner.as_deposit_receipt() {
            OpReceipt::Deposit(OpDepositReceipt {
                inner: alloy::consensus::Receipt {
                    status: deposit_receipt.status_or_post_state(),
                    cumulative_gas_used: deposit_receipt.cumulative_gas_used(),
                    logs,
                },
                deposit_nonce: deposit_receipt.deposit_nonce,
                deposit_receipt_version: deposit_receipt.deposit_receipt_version,
            })
        } else {
            let inner = receipt.inner.inner.as_receipt().unwrap();
            let exec_receipt = alloy::consensus::Receipt {
                status: inner.status,
                cumulative_gas_used: inner.cumulative_gas_used,
                logs,
            };

            match receipt.inner.inner.tx_type() {
                OpTxType::Legacy => OpReceipt::Legacy(exec_receipt),
                OpTxType::Eip2930 => OpReceipt::Eip2930(exec_receipt),
                OpTxType::Eip1559 => OpReceipt::Eip1559(exec_receipt),
                OpTxType::Eip7702 => OpReceipt::Eip7702(exec_receipt),
                OpTxType::Deposit => unreachable!(),
            }
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

    fn recover_signers(block: &OpBlock) -> Vec<VerifyingKey> {
        block
            .body
            .transactions()
            .filter(|tx| !matches!(tx.tx_type(), OpTxType::Deposit))
            .map(|tx| {
                tx.signature()
                    .recover_from_prehash(&op_signature_hash(&tx))
                    .unwrap()
            })
            .collect()
    }
}
