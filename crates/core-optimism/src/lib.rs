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

use anyhow::Context;
use k256::ecdsa::signature::hazmat::PrehashVerifier;
use k256::ecdsa::VerifyingKey;
use op_alloy_consensus::OpTxEnvelope;
use reth_chainspec::NamedChain;
use reth_codecs::alloy::transaction::Envelope;
use reth_consensus::{Consensus, HeaderValidator};
use reth_evm::execute::Executor;
use reth_evm::ConfigureEvm;
use reth_optimism_chainspec::{
    OpChainSpec, BASE_MAINNET, BASE_SEPOLIA, OP_DEV, OP_MAINNET, OP_SEPOLIA,
};
use reth_optimism_consensus::OpBeaconConsensus;
use reth_optimism_evm::OpExecutorProvider;
use reth_optimism_primitives::{OpBlock, OpReceipt, OpTransactionSigned};
use reth_primitives::{Header, RecoveredBlock, SealedBlock, SealedHeader};
use reth_revm::db::BundleState;
use reth_revm::primitives::alloy_primitives::{BlockNumber, Sealable};
use reth_revm::primitives::{Address, B256, U256};
use reth_storage_errors::provider::ProviderError;
use std::fmt::Display;
use std::mem::take;
use std::sync::Arc;
use zeth_core::db::memory::MemoryDB;
use zeth_core::db::trie::TrieDB;
use zeth_core::driver::CoreDriver;
use zeth_core::stateless::client::StatelessClient;
use zeth_core::stateless::execute::ExecutionStrategy;
use zeth_core::stateless::finalize::{MemoryDbFinalizationStrategy, TrieDbFinalizationStrategy};
use zeth_core::stateless::initialize::{
    MemoryDbInitializationStrategy, TrieDbInitializationStrategy,
};
use zeth_core::stateless::validate::ValidationStrategy;

pub struct OpRethStatelessClient;

impl StatelessClient<OpRethCoreDriver, MemoryDB> for OpRethStatelessClient {
    type Initialization = MemoryDbInitializationStrategy;
    type Validation = OpRethValidationStrategy;
    type Execution = OpRethExecutionStrategy;
    type Finalization = MemoryDbFinalizationStrategy;
}

impl StatelessClient<OpRethCoreDriver, TrieDB> for OpRethStatelessClient {
    type Initialization = TrieDbInitializationStrategy;
    type Validation = OpRethValidationStrategy;
    type Execution = OpRethExecutionStrategy;
    type Finalization = TrieDbFinalizationStrategy;
}

pub struct OpRethValidationStrategy;

impl<Database: 'static> ValidationStrategy<OpRethCoreDriver, Database>
    for OpRethValidationStrategy
{
    fn validate_header(
        chain_spec: Arc<OpChainSpec>,
        block: &mut OpBlock,
        parent_header: &mut Header,
        _total_difficulty: &mut U256,
    ) -> anyhow::Result<()> {
        // Instantiate consensus engine
        let consensus = OpBeaconConsensus::new(chain_spec);
        // Validate header (todo: seal beforehand to save rehashing costs)
        let sealed_block = SealedBlock::seal_slow(take(block));
        consensus
            .validate_header(sealed_block.sealed_header())
            .context("validate_header")?;
        // Validate header w.r.t. parent
        let sealed_parent_header = {
            let (parent_header, parent_header_seal) = take(parent_header).seal_slow().into_parts();
            SealedHeader::new(parent_header, parent_header_seal)
        };
        consensus
            .validate_header_against_parent(sealed_block.sealed_header(), &sealed_parent_header)
            .context("validate_header_against_parent")?;
        // Check pre-execution block conditions
        consensus
            .validate_block_pre_execution(&sealed_block)
            .context("validate_block_pre_execution")?;
        // Return values
        *block = sealed_block.unseal();
        *parent_header = sealed_parent_header.unseal();
        Ok(())
    }
}

pub struct OpRethExecutionStrategy;

impl<Database: reth_evm::Database> ExecutionStrategy<OpRethCoreDriver, Database>
    for OpRethExecutionStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error: Into<ProviderError> + Display,
{
    fn execute_transactions(
        chain_spec: Arc<OpChainSpec>,
        block: &mut OpBlock,
        signers: &[VerifyingKey],
        _total_difficulty: &mut U256,
        db: &mut Option<Database>,
    ) -> anyhow::Result<BundleState> {
        // Instantiate execution engine using database
        let evm_config = OpExecutorProvider::optimism(chain_spec.clone());
        let mut executor = evm_config.batch_executor(db.take().expect("Missing database"));
        // Verify the transaction signatures and compute senders
        let mut vk_it = signers.iter();
        let mut senders = Vec::with_capacity(block.body.transactions.len());
        for (i, tx) in block.body.transactions().enumerate() {
            let sender = if let Some(deposit) = tx.as_deposit() {
                // Deposit transactions are unsigned and contain the sender
                deposit.from
            } else {
                let vk = vk_it.next().unwrap();
                let sig = tx.signature();

                sig.to_k256()
                    .and_then(|sig| vk.verify_prehash(op_signature_hash(&tx).as_slice(), &sig))
                    .with_context(|| format!("invalid signature for tx {i}"))?;

                Address::from_public_key(vk)
            };
            senders.push(sender);
        }

        // Execute transactions
        let block_hash = block.hash_slow();
        let block_with_senders = RecoveredBlock::new(take(block), senders, block_hash);
        executor
            .execute_one(&block_with_senders)
            .context("execution failed")?;

        // Return block
        *block = block_with_senders.into_block();
        // Return bundle state
        Ok(executor.into_state().bundle_state)
    }
}

#[derive(Default, Copy, Clone, Debug)]
pub struct OpRethCoreDriver;

impl CoreDriver for OpRethCoreDriver {
    type ChainSpec = OpChainSpec;
    type Block = OpBlock;
    type Header = Header;
    type Receipt = OpReceipt;
    type Transaction = OpTransactionSigned;

    fn chain_spec(chain: &NamedChain) -> Option<Arc<Self::ChainSpec>> {
        match chain {
            NamedChain::Optimism => Some(OP_MAINNET.clone()),
            NamedChain::OptimismSepolia => Some(OP_SEPOLIA.clone()),
            NamedChain::Base => Some(BASE_MAINNET.clone()),
            NamedChain::BaseSepolia => Some(BASE_SEPOLIA.clone()),
            NamedChain::Dev => Some(OP_DEV.clone()),
            _ => None,
        }
    }

    fn parent_hash(header: &Self::Header) -> B256 {
        header.parent_hash
    }

    fn header_hash(header: &Self::Header) -> B256 {
        header.hash_slow()
    }

    fn state_root(header: &Self::Header) -> B256 {
        header.state_root
    }

    fn block_number(header: &Self::Header) -> BlockNumber {
        header.number
    }

    fn block_header(block: &Self::Block) -> &Self::Header {
        &block.header
    }

    fn block_to_header(block: Self::Block) -> Self::Header {
        block.header
    }

    fn accumulate_difficulty(total_difficulty: U256, header: &Self::Header) -> U256 {
        total_difficulty + header.difficulty
    }

    fn final_difficulty(
        _block: BlockNumber,
        total_difficulty: U256,
        _chain_spec: &Self::ChainSpec,
    ) -> U256 {
        total_difficulty
    }
}

pub fn op_signature_hash(tx: &OpTxEnvelope) -> B256 {
    match tx {
        OpTransactionSigned::Legacy(tx) => tx.signature_hash(),
        OpTransactionSigned::Eip2930(tx) => tx.signature_hash(),
        OpTransactionSigned::Eip1559(tx) => tx.signature_hash(),
        OpTransactionSigned::Eip7702(tx) => tx.signature_hash(),
        OpTransactionSigned::Deposit(_) => unreachable!(),
    }
}
