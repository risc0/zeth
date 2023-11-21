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

use core::{cell::RefCell, iter::once};

use alloy_sol_types::{sol, SolInterface};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use zeth_primitives::{
    address,
    block::Header,
    keccak::keccak,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction, TxEssence,
    },
    trie::MptNode,
    uint, Address, BlockHash, FixedBytes, RlpBytes, B256, U256,
};

#[cfg(not(target_os = "zkvm"))]
use log::info;

use crate::optimism::{
    batcher_transactions::BatcherTransactions,
    batches::Batches,
    channels::Channels,
    config::ChainConfig,
    derivation::{BlockInfo, Epoch, State, CHAIN_SPEC},
    epoch::BlockInput,
};

pub mod batcher_transactions;
pub mod batches;
pub mod channels;
pub mod config;
pub mod deposits;
pub mod derivation;
pub mod epoch;
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

pub trait BatcherDb {
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput<OptimismTxEssence>>;
    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header>;
    fn get_full_eth_block(&mut self, block_no: u64) -> Result<BlockInput<EthereumTxEssence>>;
    fn get_eth_block_header(&mut self, block_no: u64) -> Result<Header>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemDb {
    pub full_op_block: HashMap<u64, BlockInput<OptimismTxEssence>>,
    pub op_block_header: HashMap<u64, Header>,
    pub full_eth_block: HashMap<u64, BlockInput<EthereumTxEssence>>,
    pub eth_block_header: HashMap<u64, Header>,
}

impl MemDb {
    pub fn new() -> Self {
        MemDb {
            full_op_block: HashMap::new(),
            op_block_header: HashMap::new(),
            full_eth_block: HashMap::new(),
            eth_block_header: HashMap::new(),
        }
    }
}

impl Default for MemDb {
    fn default() -> Self {
        Self::new()
    }
}

impl BatcherDb for MemDb {
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput<OptimismTxEssence>> {
        let op_block = self.full_op_block.get(&block_no).unwrap();

        // Validate tx list
        {
            let mut tx_trie = MptNode::default();
            for (tx_no, tx) in op_block.transactions.iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                tx_trie.insert_rlp(&trie_key, tx)?;
            }
            if tx_trie.hash() != op_block.block_header.transactions_root {
                bail!("Invalid op block transaction data!")
            }
        }

        Ok(op_block.clone())
    }

    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header> {
        Ok(self.op_block_header.get(&block_no).unwrap().clone())
    }

    fn get_full_eth_block(&mut self, block_no: u64) -> Result<BlockInput<EthereumTxEssence>> {
        let eth_block = self.full_eth_block.get(&block_no).unwrap();

        // Validate tx list
        {
            let mut tx_trie = MptNode::default();
            for (tx_no, tx) in eth_block.transactions.iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                tx_trie.insert_rlp(&trie_key, tx)?;
            }
            if tx_trie.hash() != eth_block.block_header.transactions_root {
                bail!("Invalid eth block transaction data!")
            }
        }

        // Validate receipts
        if eth_block.receipts.is_some() {
            let mut receipt_trie = MptNode::default();
            for (tx_no, receipt) in eth_block.receipts.as_ref().unwrap().iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                receipt_trie.insert_rlp(&trie_key, receipt)?;
            }
            if receipt_trie.hash() != eth_block.block_header.receipts_root {
                bail!("Invalid eth block receipt data!")
            }
        } else {
            let can_contain_deposits = deposits::can_contain(
                &CHAIN_SPEC.deposit_contract,
                &eth_block.block_header.logs_bloom,
            );
            let can_contain_config = system_config::can_contain(
                &CHAIN_SPEC.system_config_contract,
                &eth_block.block_header.logs_bloom,
            );
            assert!(!can_contain_deposits);
            assert!(!can_contain_config);
        }

        Ok(eth_block.clone())
    }

    fn get_eth_block_header(&mut self, block_no: u64) -> Result<Header> {
        Ok(self.eth_block_header.get(&block_no).unwrap().clone())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveInput {
    pub mem_db: MemDb,
    pub head_block_no: u64,
    pub block_count: u64,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeriveOutput {
    pub head_block_hash: BlockHash,
    pub derived_blocks: Vec<BlockHash>,
}

impl DeriveOutput {
    pub fn new(head_block_hash: BlockHash) -> Self {
        DeriveOutput {
            head_block_hash,
            derived_blocks: Vec::new(),
        }
    }

    pub fn push(&mut self, l2_hash: BlockHash) {
        self.derived_blocks.push(l2_hash);
    }
}

pub fn derive<D: BatcherDb>(
    db: &mut D,
    head_block_no: u64,
    block_count: u64,
) -> Result<DeriveOutput> {
    let mut op_block_no = head_block_no;

    // read system config from op_head (seq_no/epoch_no..etc)
    let op_head = db.get_full_op_block(op_block_no)?;
    let op_head_hash = op_head.block_header.hash();

    let mut derive_output = DeriveOutput::new(op_head_hash);

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
    let mut eth_block_no = set_l1_block_values.number;
    let mut op_block_seq_no = set_l1_block_values.sequence_number;
    let mut op_chain_config = ChainConfig::optimism();
    op_chain_config.system_config.batch_sender =
        Address::from_slice(&set_l1_block_values.batcher_hash.as_slice()[12..]);
    op_chain_config.system_config.l1_fee_overhead = set_l1_block_values.l1_fee_overhead;
    op_chain_config.system_config.l1_fee_scalar = set_l1_block_values.l1_fee_scalar;

    #[cfg(not(target_os = "zkvm"))]
    {
        info!("Fetch eth head {}", eth_block_no);
    }
    let eth_head = db.get_eth_block_header(eth_block_no)?;
    if eth_head.hash() != set_l1_block_values.hash.as_slice() {
        bail!("Ethereum head block hash mismatch.")
    }
    let op_state = RefCell::new(State {
        current_l1_block_number: eth_block_no,
        current_l1_block_hash: BlockHash::from(eth_head.hash()),
        safe_head: BlockInfo {
            hash: op_head_hash,
            timestamp: op_head.block_header.timestamp.try_into().unwrap(),
        },
        epoch: Epoch {
            number: eth_block_no,
            hash: eth_head.hash(),
            timestamp: eth_head.timestamp.try_into().unwrap(),
        },
        next_epoch: None,
    });
    let op_buffer_queue = VecDeque::new();
    let op_buffer = RefCell::new(op_buffer_queue);
    let mut op_system_config = op_chain_config.system_config.clone();
    let mut op_batches = Batches::new(
        Channels::new(BatcherTransactions::new(&op_buffer), &op_chain_config),
        &op_state,
        &op_chain_config,
    );
    let mut op_epoch_queue = VecDeque::new();
    let mut eth_block_inputs = vec![];
    let mut op_epoch_deposit_block_ptr = 0usize;
    let mut prev_eth_block_hash = None;
    let target_block_no = head_block_no + block_count;
    while op_block_no < target_block_no {
        #[cfg(not(target_os = "zkvm"))]
        {
            info!(
                "Process op block {} as of epoch {}",
                op_block_no, eth_block_no
            );
        }

        // Get the next Eth block
        let eth_block = db
            .get_full_eth_block(eth_block_no)
            .context("block not found")?;
        let eth_block_hash = eth_block.block_header.hash();

        // Ensure block has correct parent
        if let Some(prev_eth_block_hash) = prev_eth_block_hash {
            assert_eq!(eth_block.block_header.parent_hash, prev_eth_block_hash);
        }
        prev_eth_block_hash = Some(eth_block_hash);

        let epoch = Epoch {
            number: eth_block_no,
            hash: eth_block_hash,
            timestamp: eth_block.block_header.timestamp.try_into().unwrap(),
        };
        op_epoch_queue.push_back(epoch);
        deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

        // derive batches from eth block
        if eth_block.receipts.is_some() {
            #[cfg(not(target_os = "zkvm"))]
            {
                info!("Process config and batches");
            }
            // update the system config
            op_system_config
                .update(&op_chain_config, &eth_block)
                .context("failed to update system config")?;
            // process all batcher transactions
            BatcherTransactions::process(
                op_chain_config.batch_inbox,
                op_system_config.batch_sender,
                eth_block.block_header.number,
                &eth_block.transactions,
                &op_buffer,
            )
            .context("failed to create batcher transactions")?;
        };

        eth_block_inputs.push(eth_block);

        // derive op blocks from batches
        op_state.borrow_mut().current_l1_block_number = eth_block_no;
        for op_batch in op_batches.by_ref() {
            if op_block_no == target_block_no {
                break;
            }

            #[cfg(not(target_os = "zkvm"))]
            {
                info!(
                    "derived batch: t={}, ph={:?}, e={}, tx={}",
                    op_batch.essence.timestamp,
                    op_batch.essence.parent_hash,
                    op_batch.essence.epoch_num,
                    op_batch.essence.transactions.len(),
                );
            }

            // Manage current epoch number and extract deposits
            let deposits = {
                let mut op_state_ref = op_state.borrow_mut();
                if op_batch.essence.epoch_num == op_state_ref.epoch.number + 1 {
                    op_state_ref.epoch = op_state_ref
                        .next_epoch
                        .take()
                        .expect("dequeued future batch without next epoch!");
                    op_block_seq_no = 0;

                    op_epoch_deposit_block_ptr += 1;
                    let deposit_block_input = &eth_block_inputs[op_epoch_deposit_block_ptr];
                    if deposit_block_input.block_header.number != op_batch.essence.epoch_num {
                        bail!("Invalid epoch number!")
                    };
                    #[cfg(not(target_os = "zkvm"))]
                    {
                        info!(
                            "Extracting deposits from block {} for batch with epoch {}",
                            deposit_block_input.block_header.number, op_batch.essence.epoch_num
                        );
                    }
                    let deposits =
                        deposits::extract_transactions(&op_chain_config, deposit_block_input)?;
                    #[cfg(not(target_os = "zkvm"))]
                    {
                        info!("Extracted {} deposits", deposits.len());
                    }
                    Some(deposits)
                } else {
                    #[cfg(not(target_os = "zkvm"))]
                    {
                        info!("No deposits found!");
                    }
                    op_block_seq_no += 1;
                    None
                }
            };

            deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

            // Process block transactions
            let mut op_state = op_state.borrow_mut();
            if op_batch.essence.parent_hash == op_state.safe_head.hash {
                op_block_no += 1;

                #[cfg(not(target_os = "zkvm"))]
                {
                    info!("Fetch op block {}", op_block_no);
                }
                let new_op_head = db
                    .get_op_block_header(op_block_no)
                    .context("block not found")?;

                // Verify new op head has the expected parent
                assert_eq!(new_op_head.parent_hash, op_state.safe_head.hash);

                // Verify new op head has the expected block number
                assert_eq!(new_op_head.number, op_block_no);

                // Verify that the new op head transactions are consistent with the batch transactions
                {
                    let system_tx = {
                        let eth_block_header =
                            &eth_block_inputs[op_epoch_deposit_block_ptr].block_header;
                        let batcher_hash = {
                            let all_zero: FixedBytes<12> = FixedBytes([0_u8; 12]);
                            all_zero.concat_const::<20, 32>(op_system_config.batch_sender.0)
                        };
                        let set_l1_block_values = OpSystemInfo::OpSystemInfoCalls::setL1BlockValues(
                            OpSystemInfo::setL1BlockValuesCall {
                                number: eth_block_header.number,
                                timestamp: eth_block_header.timestamp.try_into().unwrap(),
                                basefee: eth_block_header.base_fee_per_gas,
                                hash: eth_block_header.hash(),
                                sequence_number: op_block_seq_no,
                                batcher_hash,
                                l1_fee_overhead: op_system_config.l1_fee_overhead,
                                l1_fee_scalar: op_system_config.l1_fee_scalar,
                            },
                        );
                        let source_hash: B256 = {
                            let source_hash_sequencing = keccak(
                                &[
                                    op_batch.essence.epoch_hash.to_vec(),
                                    U256::from(op_block_seq_no).to_be_bytes_vec(),
                                ]
                                .concat(),
                            );
                            keccak(
                                &[
                                    [0u8; 31].as_slice(),
                                    [1u8].as_slice(),
                                    source_hash_sequencing.as_slice(),
                                ]
                                .concat(),
                            )
                            .into()
                        };
                        Transaction {
                            essence: OptimismTxEssence::OptimismDeposited(
                                TxEssenceOptimismDeposited {
                                    source_hash,
                                    from: address!("deaddeaddeaddeaddeaddeaddeaddeaddead0001"),
                                    to: TransactionKind::Call(address!(
                                        "4200000000000000000000000000000000000015"
                                    )),
                                    mint: Default::default(),
                                    value: Default::default(),
                                    gas_limit: uint!(1_000_000_U256),
                                    is_system_tx: false,
                                    data: set_l1_block_values.abi_encode().into(),
                                },
                            ),
                            signature: Default::default(),
                        }
                    };

                    let derived_transactions: Vec<_> = once(system_tx.to_rlp())
                        .chain(
                            deposits
                                .unwrap_or_default()
                                .into_iter()
                                .map(|tx| tx.to_rlp()),
                        )
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

                op_state.safe_head = BlockInfo {
                    hash: new_op_head.hash(),
                    timestamp: new_op_head.timestamp.try_into().unwrap(),
                };
                #[cfg(not(target_os = "zkvm"))]
                {
                    info!(
                        "derived l2 block {} w/ hash {}",
                        new_op_head.number,
                        new_op_head.hash()
                    );
                }

                derive_output.push(op_state.safe_head.hash);
            } else {
                #[cfg(not(target_os = "zkvm"))]
                {
                    info!("skipped batch w/ timestamp {}", op_batch.essence.timestamp);
                }
            }
        }

        eth_block_no += 1;
    }
    Ok(derive_output)
}

pub fn deque_next_epoch_if_none(
    op_state: &RefCell<State>,
    op_epoch_queue: &mut VecDeque<Epoch>,
) -> anyhow::Result<()> {
    let mut op_state = op_state.borrow_mut();
    if op_state.next_epoch.is_none() {
        while let Some(next_epoch) = op_epoch_queue.pop_front() {
            if next_epoch.number <= op_state.epoch.number {
                continue;
            } else if next_epoch.number == op_state.epoch.number + 1 {
                op_state.next_epoch = Some(next_epoch);
                break;
            } else {
                bail!("epoch gap!");
            }
        }
    }
    Ok(())
}
