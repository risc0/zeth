// Copyright 2023 RISC Zero, Inc.
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
#[cfg(not(target_os = "zkvm"))]
use log::info;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    batch::Batch,
    keccak::keccak,
    transactions::{
        ethereum::TransactionKind,
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction, TxEssence,
    },
    trie::MptNode,
    uint, Address, BlockHash, BlockNumber, FixedBytes, RlpBytes, B256, U256,
};

use crate::{
    consts::ONE,
    optimism::{
        batcher::{Batcher, BlockInfo},
        batcher_db::BatcherDb,
        config::ChainConfig,
    },
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
    pub op_derive_block_count: u64,
}

/// Represents the output of the derivation process.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeriveOutput {
    /// Ethereum tail block.
    pub eth_tail: (BlockNumber, BlockHash),
    /// Optimism head block.
    pub op_head: (BlockNumber, BlockHash),
    /// Derived Optimism blocks.
    pub derived_op_blocks: Vec<(BlockNumber, BlockHash)>,
}

/// Implementation of the actual derivation process.
pub struct DeriveMachine<D> {
    /// Input for the derivation process.
    pub derive_input: DeriveInput<D>,

    op_head_block_hash: BlockHash,
    op_block_no: u64,
    op_block_seq_no: u64,
    pub op_batcher: Batcher,
}

impl<D: BatcherDb> DeriveMachine<D> {
    /// Creates a new instance of DeriveMachine.
    pub fn new(chain_config: &ChainConfig, mut derive_input: DeriveInput<D>) -> Result<Self> {
        let op_block_no = derive_input.op_head_block_no;

        // read system config from op_head (seq_no/epoch_no..etc)
        let op_head = derive_input.db.get_full_op_block(op_block_no)?;
        let op_head_block_hash = op_head.block_header.hash();

        #[cfg(not(target_os = "zkvm"))]
        info!(
            "Fetched Op head (block no {}) {}",
            op_block_no, op_head_block_hash
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
        info!(
            "Fetched Eth head (block no {}) {}",
            eth_block_no, set_l1_block_values.hash
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
                BlockInfo {
                    hash: op_head_block_hash,
                    timestamp: op_head.block_header.timestamp.try_into().unwrap(),
                },
                &eth_head,
            )?
        };

        Ok(DeriveMachine {
            derive_input,
            op_head_block_hash,
            op_block_no,
            op_block_seq_no,
            op_batcher,
        })
    }

    pub fn derive(&mut self) -> Result<DeriveOutput> {
        let target_block_no =
            self.derive_input.op_head_block_no + self.derive_input.op_derive_block_count;

        let mut derived_op_blocks = Vec::new();
        let mut process_next_eth_block = false;

        while self.op_block_no < target_block_no {
            #[cfg(not(target_os = "zkvm"))]
            info!(
                "op_block_no = {}, eth_block_no = {}",
                self.op_block_no, self.op_batcher.state.current_l1_block_number
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
                    .process_l1_block(&eth_block)
                    .context("failed to create batcher transactions")?;
            }
            process_next_eth_block = true;

            // Process batches
            while let Some(op_batch) = self.op_batcher.read_batch()? {
                // Process the batch
                self.op_block_no += 1;

                #[cfg(not(target_os = "zkvm"))]
                info!(
                    "Read batch for Op block {}: timestamp={}, epoch={}, tx count={}, parent hash={:?}",
                    self.op_block_no,
                    op_batch.essence.timestamp,
                    op_batch.essence.epoch_num,
                    op_batch.essence.transactions.len(),
                    op_batch.essence.parent_hash,
                );

                // Update sequence number (and fetch deposits if start of new epoch)
                let deposits =
                    if op_batch.essence.epoch_num == self.op_batcher.state.epoch.number + 1 {
                        self.op_block_seq_no = 0;
                        self.op_batcher.state.do_next_epoch()?;

                        self.op_batcher
                            .state
                            .epoch
                            .deposits
                            .iter()
                            .map(|tx| tx.to_rlp())
                            .collect()
                    } else {
                        self.op_block_seq_no += 1;

                        Vec::new()
                    };

                // Obtain new Op head
                let new_op_head = {
                    let new_op_head = self
                        .derive_input
                        .db
                        .get_op_block_header(self.op_block_no)
                        .context("block not found")?;

                    // Verify new op head has the expected parent
                    assert_eq!(
                        new_op_head.parent_hash,
                        self.op_batcher.state.safe_head.hash
                    );

                    // Verify that the new op head transactions are consistent with the batch
                    // transactions
                    {
                        // From the spec:
                        // The first transaction MUST be a L1 attributes deposited transaction,
                        // followed by an array of zero-or-more user-deposited transactions.
                        let l1_attributes_tx = self.derive_l1_attributes_deposited_tx(&op_batch);
                        let derived_transactions = once(l1_attributes_tx.to_rlp())
                            .chain(deposits)
                            .chain(op_batch.essence.transactions.iter().map(|tx| tx.to_vec()))
                            .enumerate();

                        let mut tx_trie = MptNode::default();
                        for (tx_no, tx) in derived_transactions {
                            let trie_key = tx_no.to_rlp();
                            tx_trie.insert(&trie_key, tx)?;
                        }
                        if tx_trie.hash() != new_op_head.transactions_root {
                            bail!("Invalid op block transaction data! Transaction trie root does not match")
                        }
                    }

                    new_op_head
                };

                let new_op_head_hash = new_op_head.hash();

                #[cfg(not(target_os = "zkvm"))]
                info!(
                    "Derived Op block {} w/ hash {}",
                    new_op_head.number, new_op_head_hash
                );

                self.op_batcher.state.safe_head = BlockInfo {
                    hash: new_op_head_hash,
                    timestamp: new_op_head.timestamp.try_into().unwrap(),
                };

                derived_op_blocks.push((new_op_head.number, new_op_head_hash));

                if self.op_block_no == target_block_no {
                    break;
                }
            }
        }

        Ok(DeriveOutput {
            eth_tail: (
                self.op_batcher.state.current_l1_block_number,
                self.op_batcher.state.current_l1_block_hash,
            ),
            op_head: (self.derive_input.op_head_block_no, self.op_head_block_hash),
            derived_op_blocks,
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
            let l1_block_hash = op_batch.essence.epoch_hash.0;
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
