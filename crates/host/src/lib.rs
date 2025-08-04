// Copyright 2025 RISC Zero, Inc.
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

use alloy::{
    eips::BlockId,
    primitives::B256,
    providers::{Provider, ext::DebugApi},
    rpc::types::debug::ExecutionWitness,
    signers::k256,
};
use alloy_chains::NamedChain;
use anyhow::{Context, Result, bail};
use guests::{HOLESKY_ELF, MAINNET_ELF, SEPOLIA_ELF};
use k256::ecdsa::VerifyingKey;
use reth_chainspec::{ChainSpec, EthChainSpec};
use reth_ethereum_primitives::TransactionSigned;
use risc0_zkvm::{Digest, ExecutorEnvBuilder, Receipt, compute_image_id, default_prover};
use std::sync::Arc;
use zeth_core::Input;

/// Processes Ethereum blocks, including creating inputs, validating, and proving.
pub struct BlockProcessor<P> {
    /// The provider for fetching data from the Ethereum network.
    provider: Arc<P>,
    /// The chain specification.
    chain_spec: Arc<ChainSpec>,
}

impl<P> Clone for BlockProcessor<P> {
    fn clone(&self) -> Self {
        Self { provider: Arc::clone(&self.provider), chain_spec: Arc::clone(&self.chain_spec) }
    }
}

impl<P: Provider + DebugApi> BlockProcessor<P> {
    /// Creates a new BlockProcessor.
    ///
    /// This will make a network call to determine the chain ID and select the appropriate chain
    /// specification.
    pub async fn new(provider: P) -> Result<Self> {
        let chain_id = provider.get_chain_id().await.context("eth_chainId failed")?;
        let chain = chain_id.try_into().context("invalid chain ID")?;
        let chain_spec = match chain {
            NamedChain::Mainnet => reth_chainspec::MAINNET.clone(),
            NamedChain::Sepolia => reth_chainspec::SEPOLIA.clone(),
            NamedChain::Holesky => reth_chainspec::HOLESKY.clone(),
            NamedChain::Hoodi => reth_chainspec::HOODI.clone(),
            chain => bail!("unsupported chain: {chain}"),
        };

        Ok(Self { provider: provider.into(), chain_spec })
    }

    /// Returns the underlying provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Returns the named chain identifier.
    pub fn chain(&self) -> NamedChain {
        // This unwrap is safe because the constructor ensures a valid named chain.
        self.chain_spec.chain().named().unwrap()
    }

    /// Returns the guest program ELF and its corresponding image ID for the current chain.
    pub fn elf(&self) -> Result<(&'static [u8], Digest)> {
        let elf = match self.chain() {
            NamedChain::Mainnet => MAINNET_ELF,
            NamedChain::Sepolia => SEPOLIA_ELF,
            NamedChain::Holesky => HOLESKY_ELF,
            chain => bail!("unsupported chain for proving: {chain}"),
        };
        let image_id = compute_image_id(elf).context("failed to compute image id")?;

        Ok((elf, image_id))
    }

    /// Fetches the necessary data from the RPC endpoint to create the input.
    pub async fn create_input(&self, block: impl Into<BlockId>) -> Result<(Input, B256)> {
        let block_id = block.into();
        let rpc_block = self
            .provider
            .get_block(block_id)
            .full()
            .await?
            .with_context(|| format!("block {block_id} not found"))?;
        let witness = self.provider.debug_execution_witness(rpc_block.number().into()).await?;
        let block_hash = rpc_block.header.hash_slow();
        let block = reth_ethereum_primitives::Block::from(rpc_block);
        let signers = recover_signers(block.body.transactions())?;

        Ok((
            Input {
                block,
                signers,
                witness: ExecutionWitness {
                    state: witness.state,
                    codes: witness.codes,
                    keys: vec![], // keys are not used
                    headers: witness.headers,
                },
            },
            block_hash,
        ))
    }

    /// Validates the block execution on the host machine.
    pub fn validate(&self, input: Input) -> Result<B256> {
        let config = zeth_core::EthEvmConfig::new(self.chain_spec.clone());
        let hash = zeth_core::validate_block(input, config)?;

        Ok(hash)
    }

    /// Generates a RISC Zero proof of block execution.
    ///
    /// This method is computationally intensive and is run on a blocking thread.
    pub async fn prove(&self, input: Input) -> Result<(Receipt, Digest)> {
        let (elf, image_id) = self.elf()?;

        // prove in a blocking thread using the default prover
        let info = tokio::task::spawn_blocking(move || {
            let env = ExecutorEnvBuilder::default().write(&input)?.build()?;
            default_prover().prove(env, elf)
        })
        .await
        .context("proving task panicked")??;

        Ok((info.receipt, image_id))
    }
}

/// Serializes the input into a byte slice suitable for the RISC Zero ZKVM.
///
/// The ZKVM guest expects aligned words, and this function handles the conversion
/// from a struct to a raw byte vector.
pub fn to_zkvm_input_bytes(input: &Input) -> Result<Vec<u8>> {
    let words = risc0_zkvm::serde::to_vec(input)?;
    let bytes = bytemuck::cast_slice(words.as_slice());
    Ok(bytes.to_vec())
}

/// Recovers the signing [`VerifyingKey`] from each transaction's signature.
pub fn recover_signers<'a, I>(txs: I) -> Result<Vec<VerifyingKey>>
where
    I: IntoIterator<Item = &'a TransactionSigned>,
{
    txs.into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.signature()
                .recover_from_prehash(&tx.signature_hash())
                .with_context(|| format!("failed to recover signature for tx #{i}"))
        })
        .collect::<Result<Vec<_>, _>>()
}
