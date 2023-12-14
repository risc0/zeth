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
use anyhow::{bail, Context, Result};
#[cfg(not(target_os = "zkvm"))]
use log::info;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    address,
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

use crate::optimism::{
    batcher::{Batcher, BlockInfo, Epoch, State},
    batcher_db::BatcherDb,
    config::ChainConfig,
};

pub mod batcher;
pub mod batcher_channel;
pub mod batcher_db;
pub mod composition;
pub mod config;
pub mod deposits;
pub mod system_config;

sol! {
    #[derive(Debug)]
    interface OpSystemInfo {
        function setL1BlockValues(
            uint64 number,
            uint64 timestamp,
            uint256 basefee,
            bytes32 hash,
            uint64 sequence_number,
            bytes32 batcher_hash,
            uint256 l1_fee_overhead,
            uint256 l1_fee_scalar
        );
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveInput<D> {
    pub db: D,
    pub op_head_block_no: u64,
    pub op_derive_block_count: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeriveOutput {
    pub eth_tail: (BlockNumber, BlockHash),
    pub op_head: (BlockNumber, BlockHash),
    pub derived_op_blocks: Vec<(BlockNumber, BlockHash)>,
}

pub struct DeriveMachine<D> {
    pub derive_input: DeriveInput<D>,
    op_head_block_hash: BlockHash,
    op_block_no: u64,
    op_block_seq_no: u64,
    op_batcher: Batcher,
    pub eth_block_no: u64,
}

impl<D: BatcherDb> DeriveMachine<D> {
    pub fn new(mut derive_input: DeriveInput<D>) -> Result<Self> {
        let op_block_no = derive_input.op_head_block_no;

        // read system config from op_head (seq_no/epoch_no..etc)
        let op_head = derive_input.db.get_full_op_block(op_block_no)?;
        let op_head_block_hash = op_head.block_header.hash();

        #[cfg(not(target_os = "zkvm"))]
        info!(
            "Fetched Op head (block no {}) {}",
            derive_input.op_head_block_no, op_head_block_hash
        );

        let set_l1_block_values = {
            let system_tx_data = op_head
                .transactions
                .first()
                .unwrap()
                .essence
                .data()
                .to_vec();
            let call = OpSystemInfo::OpSystemInfoCalls::abi_decode(&system_tx_data, true)
                .expect("Could not decode call data");
            match call {
                OpSystemInfo::OpSystemInfoCalls::setL1BlockValues(x) => x,
            }
        };

        let op_block_seq_no = set_l1_block_values.sequence_number;

        let eth_block_no = set_l1_block_values.number;
        let eth_head = derive_input.db.get_eth_block_header(eth_block_no)?;
        let eth_head_hash = eth_head.hash();
        if eth_head_hash != set_l1_block_values.hash.as_slice() {
            bail!("Ethereum head block hash mismatch.")
        }
        #[cfg(not(target_os = "zkvm"))]
        info!(
            "Fetched Eth head (block no {}) {}",
            eth_block_no, set_l1_block_values.hash
        );

        let op_batcher = {
            let mut op_chain_config = ChainConfig::optimism();
            op_chain_config.system_config.batch_sender =
                Address::from_slice(&set_l1_block_values.batcher_hash.as_slice()[12..]);
            op_chain_config.system_config.l1_fee_overhead = set_l1_block_values.l1_fee_overhead;
            op_chain_config.system_config.l1_fee_scalar = set_l1_block_values.l1_fee_scalar;

            Batcher::new(
                op_chain_config,
                State::new(
                    eth_block_no,
                    eth_head_hash,
                    BlockInfo {
                        hash: op_head_block_hash,
                        timestamp: op_head.block_header.timestamp.try_into().unwrap(),
                    },
                    Epoch {
                        number: eth_block_no,
                        hash: eth_head_hash,
                        timestamp: eth_head.timestamp.try_into().unwrap(),
                        base_fee_per_gas: eth_head.base_fee_per_gas,
                        deposits: Vec::new(),
                    },
                ),
            )
        };

        Ok(DeriveMachine {
            derive_input,
            op_head_block_hash,
            op_block_no,
            op_block_seq_no,
            op_batcher,
            eth_block_no,
        })
    }

    pub fn derive(&mut self) -> Result<DeriveOutput> {
        let target_block_no =
            self.derive_input.op_head_block_no + self.derive_input.op_derive_block_count;

        let mut derived_op_blocks = Vec::new();

        while self.op_block_no < target_block_no {
            #[cfg(not(target_os = "zkvm"))]
            info!(
                "op_block_no = {}, eth_block_no = {}",
                self.op_block_no, self.eth_block_no
            );

            // Process next Eth block
            {
                let eth_block = self
                    .derive_input
                    .db
                    .get_full_eth_block(self.eth_block_no)
                    .context("block not found")?;

                self.op_batcher
                    .process_l1_block(&eth_block)
                    .context("failed to create batcher transactions")?;

                self.eth_block_no += 1;
            }

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
                        let system_tx = self.derive_system_transaction(&op_batch);

                        let derived_transactions: Vec<_> = once(system_tx.to_rlp())
                            .chain(deposits)
                            .chain(op_batch.essence.transactions.iter().map(|tx| tx.to_vec()))
                            .collect();

                        let mut tx_trie = MptNode::default();
                        for (tx_no, tx) in derived_transactions.into_iter().enumerate() {
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

    fn derive_system_transaction(&mut self, op_batch: &Batch) -> Transaction<OptimismTxEssence> {
        let batcher_hash = {
            let all_zero: FixedBytes<12> = FixedBytes([0_u8; 12]);
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
            let source_hash_sequencing = keccak(
                [
                    op_batch.essence.epoch_hash.to_vec(),
                    U256::from(self.op_block_seq_no).to_be_bytes_vec(),
                ]
                .concat(),
            );
            keccak(
                [
                    [0u8; 31].as_slice(),
                    [1u8].as_slice(),
                    source_hash_sequencing.as_slice(),
                ]
                .concat(),
            )
            .into()
        };

        Transaction {
            essence: OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
                source_hash,
                from: address!("deaddeaddeaddeaddeaddeaddeaddeaddead0001"),
                to: TransactionKind::Call(address!("4200000000000000000000000000000000000015")),
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
