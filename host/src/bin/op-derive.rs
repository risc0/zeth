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

/*
Example usage:

RUST_LOG=info ../zeth/target/release/op-derive \
        --eth-rpc-url="https://eth-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
        --op-rpc-url="https://opt-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
        --cache \
        --block-no=110807020 \
        --blocks=2
*/

use std::cell::RefCell;

use alloy_sol_types::SolInterface;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::collections::{HashMap, VecDeque};
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::{
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        config::ChainConfig,
        deposits, deque_next_epoch_if_none,
        derivation::{BlockInfo, Epoch, State, CHAIN_SPEC},
        epoch::BlockInput,
        OpSystemInfo,
    },
};
use zeth_primitives::{
    batch::Batch,
    block::Header,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence, TxEssence},
    Address, BlockHash,
};

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, require_equals = true)]
    /// URL of the L1 RPC node.
    eth_rpc_url: Option<String>,

    #[clap(long, require_equals = true)]
    /// URL of the L2 RPC node.
    op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata/derivation")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata/derivation]
    cache: Option<String>,

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

fn eth_cache_path(cache: &Option<String>, block_no: u64) -> Option<String> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "ethereum", block_no, "json.gz"))
}

fn op_cache_path(cache: &Option<String>, block_no: u64) -> Option<String> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "optimism", block_no, "json.gz"))
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let (mut mem_db, batches) = tokio::task::spawn_blocking(move || {
        let mut rpc_db = RpcDb::new(args.eth_rpc_url, args.op_rpc_url, args.cache);
        let batches = derive(&mut rpc_db, args.block_no, args.blocks).unwrap();
        (rpc_db.get_mem_db(), batches)
    })
    .await?;

    let batches2 = derive(&mut mem_db, args.block_no, args.blocks).unwrap();
    assert_eq!(batches, batches2);

    for batch in &batches {
        println!("batch:");
        println!("  l2 parent hash: {}", batch.essence.parent_hash);
        println!("  epoch: {}", batch.essence.epoch_num);
        println!("  epoch hash: {}", batch.essence.epoch_hash);
        println!("  timestamp: {}", batch.essence.timestamp);
        println!("  tx count: {}", batch.essence.transactions.len());
    }

    Ok(())
}

pub trait BatcherDb {
    fn get_full_op_block(&mut self, query: &BlockQuery) -> Result<BlockInput<OptimismTxEssence>>;
    fn get_full_eth_block(&mut self, query: &BlockQuery) -> Result<BlockInput<EthereumTxEssence>>;
    fn get_eth_block_header(&mut self, query: &BlockQuery) -> Result<Header>;
}

pub struct MemDb {
    pub full_op_block: HashMap<BlockQuery, BlockInput<OptimismTxEssence>>,
    pub full_eth_block: HashMap<BlockQuery, BlockInput<EthereumTxEssence>>,
    pub eth_block_header: HashMap<BlockQuery, Header>,
}

impl MemDb {
    pub fn new() -> Self {
        MemDb {
            full_op_block: HashMap::new(),
            full_eth_block: HashMap::new(),
            eth_block_header: HashMap::new(),
        }
    }
}

impl BatcherDb for MemDb {
    fn get_full_op_block(&mut self, query: &BlockQuery) -> Result<BlockInput<OptimismTxEssence>> {
        Ok(self.full_op_block.get(query).unwrap().clone())
    }

    fn get_full_eth_block(&mut self, query: &BlockQuery) -> Result<BlockInput<EthereumTxEssence>> {
        Ok(self.full_eth_block.get(query).unwrap().clone())
    }

    fn get_eth_block_header(&mut self, query: &BlockQuery) -> Result<Header> {
        Ok(self.eth_block_header.get(query).unwrap().clone())
    }
}

pub struct RpcDb {
    eth_rpc_url: Option<String>,
    op_rpc_url: Option<String>,
    cache: Option<String>,
    mem_db: MemDb,
}

impl RpcDb {
    pub fn new(
        eth_rpc_url: Option<String>,
        op_rpc_url: Option<String>,
        cache: Option<String>,
    ) -> Self {
        RpcDb {
            eth_rpc_url,
            op_rpc_url,
            cache,
            mem_db: MemDb::new(),
        }
    }

    pub fn get_mem_db(self) -> MemDb {
        self.mem_db
    }
}

impl BatcherDb for RpcDb {
    fn get_full_op_block(&mut self, query: &BlockQuery) -> Result<BlockInput<OptimismTxEssence>> {
        let mut provider = new_provider(
            op_cache_path(&self.cache, query.block_no),
            self.op_rpc_url.clone(),
        )?;
        let block = {
            let ethers_block = provider.get_full_block(query)?;
            BlockInput {
                block_header: ethers_block.clone().try_into().unwrap(),
                transactions: ethers_block
                    .transactions
                    .into_iter()
                    .map(|tx| tx.try_into().unwrap())
                    .collect(),
                receipts: None,
            }
        };
        self.mem_db.full_op_block.insert(query.clone(), block);
        provider.save()?;
        self.mem_db.get_full_op_block(query)
    }

    fn get_full_eth_block(&mut self, query: &BlockQuery) -> Result<BlockInput<EthereumTxEssence>> {
        let mut provider = new_provider(
            eth_cache_path(&self.cache, query.block_no),
            self.eth_rpc_url.clone(),
        )?;
        let block = {
            let ethers_block = provider.get_full_block(query)?;
            let block_header: Header = ethers_block.clone().try_into().unwrap();
            // include receipts when needed
            let can_contain_deposits = zeth_lib::optimism::deposits::can_contain(
                &CHAIN_SPEC.deposit_contract,
                &block_header.logs_bloom,
            );
            let can_contain_config = zeth_lib::optimism::system_config::can_contain(
                &CHAIN_SPEC.system_config_contract,
                &block_header.logs_bloom,
            );
            let receipts = if can_contain_config || can_contain_deposits {
                let receipts = provider.get_block_receipts(query)?;
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
            BlockInput {
                block_header,
                transactions: ethers_block
                    .transactions
                    .into_iter()
                    .map(|tx| tx.try_into().unwrap())
                    .collect(),
                receipts,
            }
        };
        self.mem_db.full_eth_block.insert(query.clone(), block);
        provider.save()?;
        self.mem_db.get_full_eth_block(query)
    }

    fn get_eth_block_header(&mut self, query: &BlockQuery) -> Result<Header> {
        let mut provider = new_provider(
            eth_cache_path(&self.cache, query.block_no),
            self.eth_rpc_url.clone(),
        )?;
        let header = provider.get_partial_block(query)?.try_into()?;
        self.mem_db.eth_block_header.insert(query.clone(), header);
        provider.save()?;
        self.mem_db.get_eth_block_header(query)
    }
}

fn derive<D: BatcherDb>(db: &mut D, head_block_no: u64, block_count: u64) -> Result<Vec<Batch>> {
    let mut out_batches = Vec::new();

    let mut op_block_no = head_block_no;

    // read system config from op_head (seq_no/epoch_no..etc)
    let op_head = db.get_full_op_block(&BlockQuery {
        block_no: op_block_no,
    })?;

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
    let eth_block_hash = set_l1_block_values.hash;
    let mut op_chain_config = ChainConfig::optimism();
    op_chain_config.system_config.batch_sender =
        Address::from_slice(&set_l1_block_values.batcher_hash.as_slice()[12..]);
    op_chain_config.system_config.l1_fee_overhead = set_l1_block_values.l1_fee_overhead;
    op_chain_config.system_config.l1_fee_scalar = set_l1_block_values.l1_fee_scalar;

    println!("Fetch eth head {}", eth_block_no);
    let eth_head = db.get_eth_block_header(&BlockQuery {
        block_no: eth_block_no,
    })?;
    if eth_head.hash() != eth_block_hash.as_slice() {
        bail!("Ethereum head block hash mismatch.")
    }
    let op_state = RefCell::new(State {
        current_l1_block_number: eth_block_no,
        current_l1_block_hash: BlockHash::from(eth_head.hash()),
        safe_head: BlockInfo {
            hash: op_head.block_header.hash(),
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
    let target_block_no = head_block_no + block_count;
    while op_block_no < target_block_no {
        println!(
            "Process op block {} as of epoch {}",
            op_block_no, eth_block_no
        );

        // get the block header
        let block_query = BlockQuery {
            block_no: eth_block_no,
        };
        println!("Fetch eth block {}", eth_block_no);
        let eth_block = db
            .get_full_eth_block(&block_query)
            .context("block not found")?;

        let epoch = Epoch {
            number: eth_block_no,
            hash: eth_block.block_header.hash(),
            timestamp: eth_block.block_header.timestamp.try_into().unwrap(),
        };
        op_epoch_queue.push_back(epoch);
        deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

        // derive batches from eth block
        if eth_block.receipts.is_some() {
            println!("Process config and batches");
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
        while let Some(op_batch) = op_batches.next() {
            if op_block_no == target_block_no {
                break;
            }

            println!(
                "derived batch: t={}, ph={:?}, e={}, tx={}",
                op_batch.essence.timestamp,
                op_batch.essence.parent_hash,
                op_batch.essence.epoch_num,
                op_batch.essence.transactions.len(),
            );

            // Manage current epoch number and extract deposits
            let _deposits = {
                let mut op_state_ref = op_state.borrow_mut();
                if op_batch.essence.epoch_num == op_state_ref.epoch.number + 1 {
                    op_state_ref.epoch = op_state_ref
                        .next_epoch
                        .take()
                        .expect("dequeued future batch without next epoch!");

                    op_epoch_deposit_block_ptr += 1;
                    let deposit_block_input = &eth_block_inputs[op_epoch_deposit_block_ptr];
                    if deposit_block_input.block_header.number != op_batch.essence.epoch_num {
                        bail!("Invalid epoch number!")
                    };
                    println!(
                        "Extracting deposits from block {} for batch with epoch {}",
                        deposit_block_input.block_header.number, op_batch.essence.epoch_num
                    );
                    let deposits =
                        deposits::extract_transactions(&op_chain_config, deposit_block_input)?;
                    println!("Extracted {} deposits", deposits.len());
                    Some(deposits)
                } else {
                    println!("No deposits found!");
                    None
                }
            };

            deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;

            // Process block transactions
            let mut op_state = op_state.borrow_mut();
            if op_batch.essence.parent_hash == op_state.safe_head.hash {
                op_block_no += 1;
                // TODO: check _deposits and system tx

                let new_op_head: Header = {
                    let block_query = BlockQuery {
                        block_no: op_block_no,
                    };
                    println!("Fetch op block {}", op_block_no);
                    db.get_full_op_block(&block_query)
                        .context("block not found")?
                        .block_header
                };

                op_state.safe_head = BlockInfo {
                    hash: new_op_head.hash(),
                    timestamp: new_op_head.timestamp.try_into().unwrap(),
                };
                println!(
                    "derived l2 block {} w/ hash {}",
                    new_op_head.number,
                    new_op_head.hash()
                );

                out_batches.push(op_batch);
            } else {
                println!("skipped batch w/ timestamp {}", op_batch.essence.timestamp);
            }
        }

        eth_block_no += 1;
    }
    Ok(out_batches)
}
