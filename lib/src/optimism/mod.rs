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

use core::iter::once;

use alloy_sol_types::{sol, SolInterface};
use anyhow::{bail, ensure, Context, Result};
#[cfg(target_os = "zkvm")]
use risc0_zkvm::{guest::env, serde::to_vec, sha::Digest};
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    alloy_rlp,
    batch::Batch,
    block::Header,
    keccak::keccak,
    transactions::{
        ethereum::TransactionKind,
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction, TxEssence,
    },
    trie::MptNode,
    uint, Address, FixedBytes, RlpBytes, B256, U256,
};

#[cfg(not(target_os = "zkvm"))]
use crate::{
    builder::{BlockBuilderStrategy, OptimismStrategy},
    consts::OP_MAINNET_CHAIN_SPEC,
    host::{preflight::Preflight, provider_db::ProviderDb, ProviderFactory},
};
use crate::{
    consts::ONE,
    input::{BlockBuildInput, StateInput},
    optimism::{
        batcher::{Batcher, BlockId, L2BlockInfo},
        batcher_db::BatcherDb,
        composition::ImageId,
        config::ChainConfig,
    },
    output::BlockBuildOutput,
};

pub mod batcher;
pub mod batcher_channel;
pub mod batcher_db;
pub mod composition;
pub mod config;
pub mod deposits;
pub mod system_config;

sol! {
    /// The values stored by the L1 Attributes Predeployed Contract.
    #[derive(Debug)]
    interface OpSystemInfo {
        function setL1BlockValues(
            /// L1 block attributes.
            uint64 number,
            uint64 timestamp,
            uint256 basefee,
            bytes32 hash,
            /// Sequence number in the current epoch.
            uint64 sequence_number,
            /// A versioned hash of the current authorized batcher sender.
            bytes32 batcher_hash,
            /// The L1 fee overhead to apply to L1 cost computation of transactions.
            uint256 l1_fee_overhead,
            /// The L1 fee scalar to apply to L1 cost computation of transactions.
            uint256 l1_fee_scalar
        );
    }
}

/// Represents the input for the derivation process.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveInput<D> {
    /// Database containing the blocks.
    pub db: D,
    /// Block number of the L2 head.
    pub op_head_block_no: u64,
    /// Block count for the operation.
    pub op_derive_block_count: u32,
    /// Block building data for execution
    pub op_block_outputs: Vec<BlockBuildOutput>,
    /// Image id of block builder guest
    pub block_image_id: ImageId,
}

/// Represents the output of the derivation process.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeriveOutput {
    /// Ethereum tail block.
    pub eth_tail: BlockId,
    /// Optimism head block.
    pub op_head: BlockId,
    /// Derived Optimism blocks.
    pub derived_op_blocks: Vec<BlockId>,
    /// Image id of block builder guest
    pub block_image_id: ImageId,
}

#[cfg(target_os = "zkvm")]
type ProviderFactory = ();

/// Implementation of the actual derivation process.
pub struct DeriveMachine<D> {
    /// Input for the derivation process.
    pub derive_input: DeriveInput<D>,
    op_head_block_header: Header,
    op_block_seq_no: u64,
    pub op_batcher: Batcher,
    pub provider_factory: Option<ProviderFactory>,
}

impl<D: BatcherDb> DeriveMachine<D> {
    /// Creates a new instance of DeriveMachine.
    pub fn new(
        chain_config: &ChainConfig,
        mut derive_input: DeriveInput<D>,
        provider_factory: Option<ProviderFactory>,
    ) -> Result<Self> {
        derive_input.db.validate()?;
        #[cfg(not(target_os = "zkvm"))]
        ensure!(provider_factory.is_some(), "Missing provider factory!");

        // read system config from op_head (seq_no/epoch_no..etc)
        let op_head = derive_input
            .db
            .get_full_op_block(derive_input.op_head_block_no)?;
        let op_head_block_hash = op_head.block_header.hash();

        #[cfg(not(target_os = "zkvm"))]
        log::debug!(
            "Fetched Op head (block no {}) {}",
            derive_input.op_head_block_no,
            op_head_block_hash
        );

        // the first transaction in a block MUST be a L1 attributes deposited transaction
        let l1_attributes_tx = &op_head
            .transactions
            .first()
            .context("block is empty")?
            .essence;
        if let Err(err) = validate_l1_attributes_deposited_tx(chain_config, l1_attributes_tx) {
            bail!(
                "First transaction in block is not a valid L1 attributes deposited transaction: {}",
                err
            )
        }
        // decode the L1 attributes deposited transaction
        let set_l1_block_values = {
            let call = OpSystemInfo::OpSystemInfoCalls::abi_decode(l1_attributes_tx.data(), true)
                .context("invalid L1 attributes data")?;
            match call {
                OpSystemInfo::OpSystemInfoCalls::setL1BlockValues(x) => x,
            }
        };

        let op_block_seq_no = set_l1_block_values.sequence_number;

        // check that the correct L1 block is in the database
        let eth_block_no = set_l1_block_values.number;
        let eth_head = derive_input.db.get_full_eth_block(eth_block_no)?;
        ensure!(
            eth_head.block_header.hash() == set_l1_block_values.hash,
            "Ethereum head block hash mismatch"
        );
        #[cfg(not(target_os = "zkvm"))]
        log::debug!(
            "Fetched Eth head (block no {}) {}",
            eth_block_no,
            set_l1_block_values.hash
        );

        let op_batcher = {
            // copy the chain config and update the system config
            let mut op_chain_config = chain_config.clone();
            op_chain_config.system_config.batch_sender =
                Address::from_slice(&set_l1_block_values.batcher_hash.as_slice()[12..]);
            op_chain_config.system_config.l1_fee_overhead = set_l1_block_values.l1_fee_overhead;
            op_chain_config.system_config.l1_fee_scalar = set_l1_block_values.l1_fee_scalar;

            Batcher::new(
                op_chain_config,
                L2BlockInfo {
                    hash: op_head_block_hash,
                    timestamp: op_head.block_header.timestamp.try_into().unwrap(),
                    l1_origin: BlockId {
                        number: set_l1_block_values.number,
                        hash: set_l1_block_values.hash,
                    },
                },
                eth_head,
            )?
        };

        Ok(DeriveMachine {
            derive_input,
            op_head_block_header: op_head.block_header,
            op_block_seq_no,
            op_batcher,
            provider_factory,
        })
    }

    pub fn derive(
        &mut self,
        mut op_block_inputs: Option<&mut Vec<BlockBuildInput<OptimismTxEssence>>>,
    ) -> Result<DeriveOutput> {
        #[cfg(target_os = "zkvm")]
        op_block_inputs.take();

        ensure!(
            self.op_head_block_header.number == self.derive_input.op_head_block_no,
            "Op head block number mismatch!"
        );
        let target_block_no =
            self.derive_input.op_head_block_no + self.derive_input.op_derive_block_count as u64;

        // Save starting op_head
        let op_head = BlockId {
            number: self.op_head_block_header.number,
            hash: self.op_head_block_header.hash(),
        };

        let mut derived_op_blocks = Vec::new();
        let mut process_next_eth_block = false;

        #[cfg(target_os = "zkvm")]
        let mut op_block_output_iter =
            core::mem::take(&mut self.derive_input.op_block_outputs).into_iter();

        while self.op_head_block_header.number < target_block_no {
            #[cfg(not(target_os = "zkvm"))]
            log::trace!(
                "op_block_no = {}, eth_block_no = {}",
                self.op_head_block_header.number,
                self.op_batcher.state.current_l1_block_number
            );

            // Process next Eth block. We do this on every iteration, except the first iteration.
            // (The first iteration is handled by Batcher::new().)
            if process_next_eth_block {
                let eth_block = self
                    .derive_input
                    .db
                    .get_full_eth_block(self.op_batcher.state.current_l1_block_number + 1)
                    .context("block not found")?;

                self.op_batcher
                    .process_l1_block(eth_block)
                    .context("failed to create batcher transactions")?;
            }
            process_next_eth_block = true;

            // Process batches
            while let Some(op_batch) = self.op_batcher.read_batch()? {
                // Process the batch

                #[cfg(not(target_os = "zkvm"))]
                log::debug!(
                    "Read batch for Op block {}: timestamp={}, epoch={}, tx count={}, parent hash={:?}",
                    self.op_head_block_header.number + 1,
                    op_batch.0.timestamp,
                    op_batch.0.epoch_num,
                    op_batch.0.transactions.len(),
                    op_batch.0.parent_hash,
                );

                // Update sequence number (and fetch deposits if start of new epoch)
                let l2_safe_head = &self.op_batcher.state.safe_head;
                let deposits = if l2_safe_head.l1_origin.number != op_batch.0.epoch_num {
                    self.op_block_seq_no = 0;
                    self.op_batcher.state.do_next_epoch()?;

                    self.op_batcher.state.epoch.deposits.clone()
                } else {
                    self.op_block_seq_no += 1;

                    vec![]
                };

                let l1_epoch_header_mix_hash = self
                    .derive_input
                    .db
                    .get_full_eth_block(op_batch.0.epoch_num)
                    .context("eth block not found")?
                    .block_header
                    .mix_hash;

                // From the spec:
                // The first transaction MUST be a L1 attributes deposited transaction,
                // followed by an array of zero-or-more user-deposited transactions.
                let l1_attributes_tx = self.derive_l1_attributes_deposited_tx(&op_batch);
                // TODO: revise that skipping undecodable transactions is part of spec
                let decoded_batch_transactions: Vec<_> = op_batch
                    .0
                    .transactions
                    .iter()
                    .filter_map(|raw_tx| {
                        match Transaction::<OptimismTxEssence>::decode_bytes(raw_tx) {
                            Ok(tx) => Some(tx),
                            Err(_err) => {
                                #[cfg(not(target_os = "zkvm"))]
                                log::warn!("Skipping undecodable transaction: {:#}", _err);
                                None
                            }
                        }
                    })
                    .collect();

                let derived_transactions: Vec<_> = once(l1_attributes_tx)
                    .chain(deposits)
                    .chain(decoded_batch_transactions)
                    .collect();
                let derived_transactions_rlp = derived_transactions
                    .iter()
                    .map(alloy_rlp::encode)
                    .enumerate();

                let mut tx_trie = MptNode::default();
                for (tx_no, tx) in derived_transactions_rlp {
                    tx_trie.insert(&alloy_rlp::encode(tx_no), tx)?;
                }

                let new_op_head_input = BlockBuildInput {
                    state_input: StateInput {
                        parent_header: self.op_head_block_header.clone(),
                        beneficiary: self.op_batcher.config.sequencer_fee_vault,
                        gas_limit: self.op_batcher.config.system_config.gas_limit,
                        timestamp: U256::from(op_batch.0.timestamp),
                        extra_data: Default::default(),
                        mix_hash: l1_epoch_header_mix_hash,
                        transactions: derived_transactions,
                        withdrawals: vec![],
                    },
                    // initializing these fields is not needed here
                    parent_state_trie: Default::default(),
                    parent_storage: Default::default(),
                    contracts: vec![],
                    ancestor_headers: vec![],
                };

                // host: go run the preflight and queue up the input data (using RLP decoded
                // transactions)
                #[cfg(not(target_os = "zkvm"))]
                let op_block_output = {
                    // Create the provider DB
                    // todo: run without factory (using outputs)
                    let provider_db = ProviderDb::new(
                        self.provider_factory
                            .as_ref()
                            .unwrap()
                            .create_provider(self.op_head_block_header.number)?,
                        self.op_head_block_header.number,
                    );
                    let preflight_data = OptimismStrategy::preflight_with_local_data(
                        &OP_MAINNET_CHAIN_SPEC,
                        provider_db,
                        new_op_head_input.clone(),
                    )
                    .map(|mut headerless_preflight_data| {
                        let header = Header {
                            beneficiary: new_op_head_input.state_input.beneficiary,
                            gas_limit: new_op_head_input.state_input.gas_limit,
                            timestamp: new_op_head_input.state_input.timestamp,
                            extra_data: new_op_head_input.state_input.extra_data.clone(),
                            mix_hash: new_op_head_input.state_input.mix_hash,
                            // unnecessary
                            ..Default::default()
                        };
                        headerless_preflight_data.header = Some(header);
                        headerless_preflight_data
                    })?;

                    let executable_input: BlockBuildInput<OptimismTxEssence> =
                        preflight_data.try_into()?;
                    if let Some(ref mut inputs_vec) = op_block_inputs {
                        inputs_vec.push(executable_input.clone());
                    }

                    OptimismStrategy::build_from(&OP_MAINNET_CHAIN_SPEC, executable_input)?
                        .with_state_hashed()
                };
                // guest: ask for receipt about provided block build output (compressed state trie
                // expected)
                #[cfg(target_os = "zkvm")]
                let op_block_output = {
                    let output = op_block_output_iter.next().unwrap();
                    // A valid receipt should be provided for block building results
                    let builder_journal =
                        to_vec(&output).expect("Failed to encode builder journal");
                    env::verify(
                        Digest::from(self.derive_input.block_image_id),
                        &builder_journal,
                    )
                    .expect("Failed to validate block build output");
                    output
                };

                // Ensure that the output came from the expected input
                ensure!(
                    new_op_head_input.state_input.hash() == op_block_output.state_input_hash(),
                    "Invalid state input hash"
                );
                match op_block_output {
                    BlockBuildOutput::SUCCESS {
                        hash: new_block_hash,
                        head: new_block_head,
                        ..
                    } => {
                        // obtain verified op block header
                        #[cfg(not(target_os = "zkvm"))]
                        log::info!(
                            "Derived Op block {} w/ hash {}",
                            new_block_head.number,
                            new_block_hash
                        );

                        self.op_batcher.state.safe_head = L2BlockInfo {
                            hash: new_block_hash,
                            timestamp: new_block_head.timestamp.try_into().unwrap(),
                            l1_origin: BlockId {
                                number: self.op_batcher.state.epoch.number,
                                hash: self.op_batcher.state.epoch.hash,
                            },
                        };

                        derived_op_blocks.push(BlockId {
                            number: new_block_head.number,
                            hash: new_block_hash,
                        });
                        self.op_head_block_header = new_block_head;

                        if self.op_head_block_header.number == target_block_no {
                            break;
                        }
                    }
                    BlockBuildOutput::FAILURE { .. } => {
                        #[cfg(not(target_os = "zkvm"))]
                        log::warn!("Failed to build block from batch");
                    }
                };
            }
        }

        Ok(DeriveOutput {
            eth_tail: BlockId {
                number: self.op_batcher.state.current_l1_block_number,
                hash: self.op_batcher.state.current_l1_block_hash,
            },
            op_head,
            derived_op_blocks,
            block_image_id: self.derive_input.block_image_id,
        })
    }

    fn derive_l1_attributes_deposited_tx(
        &mut self,
        op_batch: &Batch,
    ) -> Transaction<OptimismTxEssence> {
        let batcher_hash = {
            let all_zero: FixedBytes<12> = FixedBytes::ZERO;
            all_zero.concat_const::<20, 32>(self.op_batcher.config.system_config.batch_sender.0)
        };

        let set_l1_block_values =
            OpSystemInfo::OpSystemInfoCalls::setL1BlockValues(OpSystemInfo::setL1BlockValuesCall {
                number: self.op_batcher.state.epoch.number,
                timestamp: self.op_batcher.state.epoch.timestamp,
                basefee: self.op_batcher.state.epoch.base_fee_per_gas,
                hash: self.op_batcher.state.epoch.hash,
                sequence_number: self.op_block_seq_no,
                batcher_hash,
                l1_fee_overhead: self.op_batcher.config.system_config.l1_fee_overhead,
                l1_fee_scalar: self.op_batcher.config.system_config.l1_fee_scalar,
            });

        let source_hash: B256 = {
            let l1_block_hash = op_batch.0.epoch_hash.0;
            let seq_number = U256::from(self.op_block_seq_no).to_be_bytes::<32>();
            let source_hash_sequencing = keccak([l1_block_hash, seq_number].concat());
            keccak([ONE.to_be_bytes::<32>(), source_hash_sequencing].concat()).into()
        };
        let config = &self.op_batcher.config;

        Transaction {
            essence: OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
                source_hash,
                from: config.l1_attributes_depositor,
                to: TransactionKind::Call(config.l1_attributes_contract),
                mint: Default::default(),
                value: Default::default(),
                gas_limit: uint!(1_000_000_U256),
                is_system_tx: false,
                data: set_l1_block_values.abi_encode().into(),
            }),
            signature: Default::default(),
        }
    }
}

fn validate_l1_attributes_deposited_tx(config: &ChainConfig, tx: &OptimismTxEssence) -> Result<()> {
    match tx {
        OptimismTxEssence::Ethereum(_) => {
            bail!("No Optimism deposit transaction");
        }
        OptimismTxEssence::OptimismDeposited(op) => {
            ensure!(
                op.from == config.l1_attributes_depositor,
                "Invalid from address"
            );
            ensure!(
                matches!(op.to, TransactionKind::Call(addr) if addr == config.l1_attributes_contract),
                "Invalid to address"
            );
            ensure!(op.mint == U256::ZERO, "Invalid mint value");
            ensure!(op.value == U256::ZERO, "Invalid value");
            ensure!(op.gas_limit == uint!(1_000_000_U256), "Invalid gas limit");
            ensure!(!op.is_system_tx, "Invalid is_system_tx value");
        }
    }

    Ok(())
}
