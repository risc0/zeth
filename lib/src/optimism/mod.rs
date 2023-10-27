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

use anyhow::bail;
use ethers_core::abi::{ParamType, Token};
use heapless::spsc::Queue;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    address,
    ethers::{from_ethers_u256, to_ethers_u256},
    keccak::keccak,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction, TxEssence,
    },
    trie::MptNode,
    uint, Address, BlockHash, Bytes, RlpBytes,
};

use crate::{
    block_builder::{ConfiguredBlockBuilder, OptimismStrategyBundle},
    consts::OP_MAINNET_CHAIN_SPEC,
    input::Input,
    optimism::{
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        config::ChainConfig,
        derivation::{BlockInfo, Epoch, State, CHAIN_SPEC},
        epoch::BlockInput,
    },
};

pub mod batcher_transactions;
pub mod batches;
pub mod channels;
pub mod config;
pub mod deposits;
pub mod derivation;
pub mod epoch;
pub mod system_config;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DerivationInput {
    pub eth_block_inputs: Vec<BlockInput<EthereumTxEssence>>,
    pub op_block_inputs: Vec<Input<OptimismTxEssence>>,
    pub op_head: BlockInput<OptimismTxEssence>,
}

impl DerivationInput {
    pub fn process(self) -> anyhow::Result<State> {
        // Initialize data
        let op_head_block_header = &self.op_head.block_header;
        let mut op_block_no = op_head_block_header.number;
        // read system config from op_head (seq_no/epoch_no..etc)
        let system_tx_data = self
            .op_head
            .transactions
            .first()
            .unwrap()
            .essence
            .data()
            .to_vec();
        let decoded_data = ethers_core::abi::decode(
            &[
                ParamType::Uint(64),       // 0 l1 number
                ParamType::Uint(64),       // 1 l1 timestamp
                ParamType::Uint(256),      // 2 l1 base fee
                ParamType::FixedBytes(32), // 3 l1 block hash
                ParamType::Uint(64),       // 4 l2 sequence number
                ParamType::FixedBytes(32), // 5 batcher hash
                ParamType::Uint(256),      // 6 l1 fee overhead
                ParamType::Uint(256),      // 7 l1 fee scalar
            ],
            &system_tx_data[4..],
        )?;
        let mut eth_block_no = decoded_data[0].clone().into_uint().unwrap().as_u64();
        let mut op_block_seq_no = decoded_data[4].clone().into_uint().unwrap().as_u64();
        let mut eth_block_hash = BlockHash::from_slice(
            decoded_data[3]
                .clone()
                .into_fixed_bytes()
                .unwrap()
                .as_slice(),
        );
        let mut op_chain_config = ChainConfig::optimism();
        op_chain_config.system_config.batch_sender = Address::from_slice(
            &decoded_data[5]
                .clone()
                .into_fixed_bytes()
                .unwrap()
                .as_slice()[12..],
        );
        op_chain_config.system_config.l1_fee_overhead =
            from_ethers_u256(decoded_data[6].clone().into_uint().unwrap());
        op_chain_config.system_config.l1_fee_scalar =
            from_ethers_u256(decoded_data[7].clone().into_uint().unwrap());
        let eth_head = &self.eth_block_inputs.first().unwrap().block_header;
        if eth_head.hash() != eth_block_hash {
            bail!("Invalid input eth head.")
        }
        let op_state = RefCell::new(State {
            current_l1_block_number: eth_block_no,
            current_l1_block_hash: eth_block_hash,
            safe_head: BlockInfo {
                hash: op_head_block_header.hash(),
                timestamp: op_head_block_header.timestamp.try_into().unwrap(),
            },
            epoch: Epoch {
                number: eth_block_no,
                hash: eth_block_hash,
                timestamp: eth_head.timestamp.try_into().unwrap(),
            },
            next_epoch: None,
        });
        let op_buffer_queue = Queue::<_, 1024>::new();
        let op_buffer = RefCell::new(op_buffer_queue);
        let mut op_system_config = op_chain_config.system_config.clone();
        let mut op_batches = Batches::new(
            Channels::new(
                BatcherTransactions::<1024, 1024>::new(&op_buffer),
                &op_chain_config,
            ),
            &op_state,
            &op_chain_config,
        );
        let mut op_epoch_queue = Queue::<_, 1024>::new();
        let mut op_epoch_deposit_block_ptr = 0usize;
        let target_block_no = op_head_block_header.number + self.op_block_inputs.len() as u64;
        let mut eth_block_iter = self.eth_block_inputs.iter();
        let mut op_block_iter = self.op_block_inputs.into_iter();
        let mut last_eth_block_hash = None;
        while op_block_no < target_block_no {
            let eth_block_input = eth_block_iter.next().unwrap();
            let eth_block_header = &eth_block_input.block_header;
            eth_block_hash = eth_block_header.hash();
            if eth_block_header.number != eth_block_no {
                bail!("Invalid input eth block sequence");
            }
            // validate eth block hash chain
            if let Some(previous_block_hash) = last_eth_block_hash {
                if previous_block_hash != eth_block_header.parent_hash {
                    bail!("Bad ethereum block parent hash");
                }
            }
            last_eth_block_hash = Some(eth_block_hash);
            let epoch = Epoch {
                number: eth_block_no,
                hash: eth_block_hash,
                timestamp: eth_block_header.timestamp.try_into().unwrap(),
            };
            op_epoch_queue.enqueue(epoch).unwrap();
            deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

            let can_contain_deposits =
                deposits::can_contain(&CHAIN_SPEC.deposit_contract, &eth_block_header.logs_bloom);
            let can_contain_config = system_config::can_contain(
                &CHAIN_SPEC.system_config_contract,
                &eth_block_header.logs_bloom,
            );

            // validate eth block tx trie root
            let mut tx_trie = MptNode::default();
            for (tx_no, tx) in eth_block_input.transactions.iter().enumerate() {
                let trie_key = tx_no.to_rlp();
                tx_trie.insert_rlp(&trie_key, tx)?;
            }
            if tx_trie.hash() != eth_block_input.block_header.transactions_root {
                bail!("Invalid eth block transaction data!")
            }

            if eth_block_input.receipts.is_some() {
                // validate eth block receipt trie root
                let mut receipt_trie = MptNode::default();
                for (tx_no, receipt) in eth_block_input
                    .receipts
                    .as_ref()
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    let trie_key = tx_no.to_rlp();
                    receipt_trie.insert_rlp(&trie_key, receipt)?;
                }
                if receipt_trie.hash() != eth_block_input.block_header.receipts_root {
                    bail!("Invalid eth block receipt data!")
                }
                // update the system config
                op_system_config.update(&op_chain_config, &eth_block_input)?;
                // process all batcher transactions
                BatcherTransactions::<1024, 1024>::process(
                    op_chain_config.batch_inbox,
                    op_system_config.batch_sender,
                    eth_block_input.block_header.number,
                    &eth_block_input.transactions,
                    &op_buffer,
                )?;
            } else if can_contain_deposits || can_contain_config {
                bail!("Missing necessary eth block receipt data!");
            }

            // derive op blocks from batches
            op_state.borrow_mut().current_l1_block_number = eth_block_no;
            op_state.borrow_mut().current_l1_block_hash = eth_block_hash;
            while let Some(op_batch) = op_batches.next() {
                if op_block_no == target_block_no {
                    break;
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
                        let deposit_block_input =
                            &self.eth_block_inputs[op_epoch_deposit_block_ptr];
                        if deposit_block_input.block_header.number != op_batch.essence.epoch_num {
                            bail!("Invalid epoch number!")
                        };

                        Some(deposits::extract_transactions(
                            &op_chain_config,
                            deposit_block_input,
                        )?)
                    } else {
                        op_block_seq_no += 1;
                        None
                    }
                };
                deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

                let mut op_state = op_state.borrow_mut();
                if op_batch.essence.parent_hash == op_state.safe_head.hash {
                    op_block_no += 1;

                    let eth_block_header =
                        &self.eth_block_inputs[op_epoch_deposit_block_ptr].block_header;
                    // run block builder with optimism strategy bundle
                    let new_op_head = {
                        // Fetch all of the initial data
                        let data = [
                            vec![0x01, 0x5d, 0x8e, 0xb9],
                            ethers_core::abi::encode(&[
                                Token::Uint(eth_block_header.number.into()),
                                Token::Uint(to_ethers_u256(eth_block_header.timestamp)),
                                Token::Uint(to_ethers_u256(eth_block_header.base_fee_per_gas)),
                                Token::FixedBytes(eth_block_header.hash().0.into()),
                                Token::Uint(op_block_seq_no.into()),
                                Token::Address(op_system_config.batch_sender.0 .0.into()),
                                Token::Uint(to_ethers_u256(op_system_config.l1_fee_overhead)),
                                Token::Uint(to_ethers_u256(op_system_config.l1_fee_scalar)),
                            ]),
                        ]
                        .concat();
                        let source_hash_sequencing = keccak(
                            &[
                                op_batch.essence.epoch_hash.to_vec(),
                                U256::from(op_block_seq_no).to_be_bytes_vec(),
                            ]
                            .concat(),
                        );
                        let source_hash = keccak(
                            &[
                                [0u8; 31].as_slice(),
                                [1u8].as_slice(),
                                source_hash_sequencing.as_slice(),
                            ]
                            .concat(),
                        );
                        let system_transaction = Transaction {
                            essence: OptimismTxEssence::OptimismDeposited(
                                TxEssenceOptimismDeposited {
                                    source_hash: source_hash.into(),
                                    from: address!("deaddeaddeaddeaddeaddeaddeaddeaddead0001"),
                                    to: TransactionKind::Call(address!(
                                        "4200000000000000000000000000000000000015"
                                    )),
                                    mint: Default::default(),
                                    value: Default::default(),
                                    gas_limit: uint!(1_000_000_U256),
                                    is_system_tx: false,
                                    data: Bytes::from(data),
                                },
                            ),
                            signature: Default::default(),
                        };

                        let op_derived_transactions: Vec<_> = once(system_transaction.to_rlp())
                            .chain(
                                deposits
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|tx| tx.to_rlp()),
                            )
                            .chain(op_batch.essence.transactions.iter().map(|tx| tx.to_vec()))
                            .collect();
                        let input = op_block_iter.next().unwrap();
                        let op_input_transactions: Vec<_> =
                            input.transactions.iter().map(|tx| tx.to_rlp()).collect();

                        if op_derived_transactions != op_input_transactions {
                            bail!("Derived transactions do not match provided input transactions!");
                        }

                        // derive
                        ConfiguredBlockBuilder::<OptimismStrategyBundle>::build_from(
                            &OP_MAINNET_CHAIN_SPEC,
                            input,
                        )?
                    };

                    if new_op_head.parent_hash != op_state.safe_head.hash {
                        bail!("Incoherent OP block parent block hash");
                    }

                    op_state.safe_head = BlockInfo {
                        hash: new_op_head.hash(),
                        timestamp: new_op_head.timestamp.try_into().unwrap(),
                    };
                }
            }

            eth_block_no += 1;
        }

        Ok(op_state.take())
    }
}

pub fn deque_next_epoch_if_none<const N: usize>(
    op_state: &RefCell<State>,
    op_epoch_queue: &mut Queue<Epoch, N>,
) -> anyhow::Result<()> {
    let mut op_state = op_state.borrow_mut();
    if op_state.next_epoch.is_none() {
        while let Some(next_epoch) = op_epoch_queue.dequeue() {
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
