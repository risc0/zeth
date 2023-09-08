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

use std::{cell::RefCell, collections::VecDeque};

use anyhow::{bail, Context, Result};
use clap::Parser;
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::{
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        config::ChainConfig,
        derivation::{BlockInfo, Epoch, State, CHAIN_SPEC},
        epoch::BlockInput,
    },
};
use zeth_primitives::block::Header;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, require_equals = true)]
    /// URL of the L1 RPC node.
    eth_rpc_url: Option<String>,

    #[clap(long, require_equals = true)]
    /// URL of the L2 RPC node.
    op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata]
    cache: Option<String>,

    #[clap(long, require_equals = true)]
    /// Epoch number (L1 Block number) of the L2 block to begin from.
    epoch_no: u64,

    #[clap(long, require_equals = true)]
    /// L2 block number to begin from
    block_no: u64,

    #[clap(long, require_equals = true)]
    /// Number of L2 blocks to provably derive.
    blocks: u64,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "20")]
    /// Runs the verification inside the zkvm executor locally. Accepts a custom maximum
    /// segment cycle count as a power of 2. [default: 20]
    local_exec: Option<usize>,

    #[clap(short, long, default_value_t = false)]
    /// Whether to submit the proving workload to Bonsai.
    submit_to_bonsai: bool,

    #[clap(short, long, require_equals = true)]
    /// Bonsai Session UUID to use for receipt verification.
    verify_bonsai_receipt_uuid: Option<String>,

    #[clap(short, long, default_value_t = false)]
    /// Whether to profile the zkVM execution
    profile: bool,
}

fn cache_file_path(cache_path: &String, network: &str, block_no: u64, ext: &str) -> String {
    format!("{}/{}/{}.{}", cache_path, network, block_no, ext)
}

fn eth_cache_path(args: &Args, block_no: u64) -> Option<String> {
    args.cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "ethereum", block_no, "json.gz"))
}

fn op_cache_path(args: &Args, block_no: u64) -> Option<String> {
    args.cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "optimism", block_no, "json.gz"))
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    tokio::task::spawn_blocking(move || get_initial_zop_data(&args)).await?;

    Ok(())
}

fn get_initial_zop_data(args: &Args) -> Result<()> {
    let mut eth_block_no = args.epoch_no;
    let mut eth_blocks = vec![];
    let mut op_block_no = args.block_no;
    // let mut op_inputs: vec![];

    // Create dynamic block derivation struct
    println!("Fetch op head {}", op_block_no);
    let op_head = new_provider(op_cache_path(&args, op_block_no), args.op_rpc_url.clone())?
        .get_partial_block(&BlockQuery {
            block_no: op_block_no,
        })?;
    println!("Fetch eth head {}", eth_block_no);
    let eth_head = new_provider(
        eth_cache_path(&args, eth_block_no),
        args.eth_rpc_url.clone(),
    )?
    .get_partial_block(&BlockQuery {
        block_no: eth_block_no,
    })?;
    let op_state = RefCell::new(State {
        current_l1_block: eth_block_no,
        safe_head: BlockInfo {
            hash: op_head.hash.unwrap().0.into(),
            timestamp: op_head.timestamp.try_into().unwrap(),
        },
        epoch: Epoch {
            number: eth_block_no,
            hash: eth_head.hash.unwrap().0.into(),
            timestamp: eth_head.timestamp.try_into().unwrap(),
        },
        next_epoch: None,
    });
    let op_buffer = RefCell::new(VecDeque::new());
    let op_chain_config = ChainConfig::optimism();
    let mut op_system_config = op_chain_config.system_config.clone();
    let mut op_batches = Batches::new(
        Channels::new(BatcherTransactions::new(&op_buffer), &op_chain_config),
        &op_state,
        &op_chain_config,
    );
    let mut op_epoch_queue = VecDeque::new();
    let target_block_no = args.block_no + args.blocks;
    while op_block_no < target_block_no {
        println!("Process op block {}", op_block_no);
        let mut eth_provider = new_provider(
            eth_cache_path(&args, eth_block_no),
            args.eth_rpc_url.clone(),
        )?;

        // get the block header
        let block_query = BlockQuery {
            block_no: eth_block_no,
        };
        println!("Fetch eth block {}", eth_block_no);
        let eth_block = eth_provider
            .get_full_block(&block_query)
            .context("block not found")?;
        let header: Header = eth_block
            .clone()
            .try_into()
            .context("invalid block header")?;

        let epoch = Epoch {
            number: eth_block_no,
            hash: eth_block.hash.unwrap().0.into(),
            timestamp: eth_block.timestamp.as_u64(),
        };
        op_epoch_queue.push_back(epoch);
        deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

        let can_contain_deposits = zeth_lib::optimism::deposits::can_contain(
            &CHAIN_SPEC.deposit_contract,
            &header.logs_bloom,
        );
        let can_contain_config = zeth_lib::optimism::system_config::can_contain(
            &CHAIN_SPEC.system_config_contract,
            &header.logs_bloom,
        );

        // include receipts when needed
        let receipts = if can_contain_config || can_contain_deposits {
            println!("Fetch eth block receipts {}", eth_block_no);
            let receipts = eth_provider
                .get_block_receipts(&block_query)
                .context("block not found")?;
            Some(
                receipts
                    .into_iter()
                    .map(|receipt| receipt.try_into())
                    .collect::<Result<Vec<_>, _>>()
                    .context("invalid receipt")?,
            )
        } else {
            None
        };

        let block_input = BlockInput {
            block_header: header,
            receipts: receipts.clone(),
            transactions: eth_block
                .transactions
                .into_iter()
                .map(|tx| tx.try_into().unwrap())
                .collect(),
        };

        // derive batches from eth block
        if let Some(ref _receipts) = receipts {
            println!("Process config and batches");
            // update the system config
            op_system_config
                .update(&op_chain_config, &block_input)
                .context("failed to update system config")?;
            // process all batcher transactions
            BatcherTransactions::process(
                op_chain_config.batch_inbox,
                op_system_config.batch_sender,
                block_input.block_header.number,
                &block_input.transactions,
                &op_buffer,
            )
            .context("failed to create batcher transactions")?;
        };

        eth_blocks.push(block_input);

        // todo: derive op blocks from batches
        op_state.borrow_mut().current_l1_block = eth_block_no;
        while let Some(op_batch) = op_batches.next() {
            if op_block_no == target_block_no {
                break;
            }

            println!(
                "derived batch: t={}, ph={:?}, e={}",
                op_batch.essence.timestamp,
                op_batch.essence.parent_hash,
                op_batch.essence.epoch_num
            );

            // Manage current epoch number
            {
                let mut op_state_ref = op_state.borrow_mut();
                if op_batch.essence.epoch_num == op_state_ref.epoch.number + 1 {
                    op_state_ref.epoch = op_state_ref
                        .next_epoch
                        .take()
                        .expect("dequeued future batch without next epoch!");
                }
            }
            deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;
            // Process block transactions
            // todo: extract deposits
            // todo: run block builder with optimism strategy bundle
            let mut op_state = op_state.borrow_mut();
            if op_batch.essence.parent_hash == op_state.safe_head.hash {
                op_block_no += 1;
                let new_op_head =
                    new_provider(op_cache_path(&args, op_block_no), args.op_rpc_url.clone())?
                        .get_partial_block(&BlockQuery {
                            block_no: op_block_no,
                        })?;
                op_state.safe_head = BlockInfo {
                    hash: new_op_head.hash.unwrap().0.into(),
                    timestamp: new_op_head.timestamp.as_u64(),
                };
                println!("derived l2 block {}", new_op_head.number.unwrap());
            } else {
                println!("skipped batch w/ timestamp {}", op_batch.essence.timestamp);
            }
        }

        eth_block_no += 1;
    }
    Ok(())
}

fn deque_next_epoch_if_none(
    op_state: &RefCell<State>,
    op_epoch_queue: &mut VecDeque<Epoch>,
) -> Result<()> {
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
