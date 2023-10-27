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

use std::{
    cell::RefCell,
    iter::{once, zip},
    time::Instant,
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use ethers_core::abi::{ParamType, Token};
use heapless::spsc::Queue;
use log::info;
use risc0_zkvm::{serde::to_vec, Executor, ExecutorEnv, FileSegmentRef};
use ruint::aliases::U256;
use tempfile::tempdir;
use zeth_guests::{OP_DERIVE_ELF, OP_DERIVE_PATH};
use zeth_lib::{
    block_builder::{ConfiguredBlockBuilder, OptimismStrategyBundle},
    consts::OP_MAINNET_CHAIN_SPEC,
    host::provider::{new_provider, BlockQuery},
    input::Input,
    optimism::{
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        config::ChainConfig,
        deposits, deque_next_epoch_if_none,
        derivation::{BlockInfo, Epoch, State, CHAIN_SPEC},
        epoch::BlockInput,
        DerivationInput,
    },
};
use zeth_primitives::{
    address,
    block::Header,
    ethers::{from_ethers_u256, to_ethers_u256},
    keccak::keccak,
    transactions::{
        ethereum::TransactionKind,
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction,
    },
    uint, Address, BlockHash, Bytes, RlpBytes,
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
    /// [default: host/testdata]
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

    let args_clone = args.clone();
    let input = tokio::task::spawn_blocking(move || get_initial_zop_data(&args_clone)).await??;

    // validate host side
    let _derived_transition = input.clone().process()?;

    // Run in the executor (if requested)
    if let Some(segment_limit_po2) = args.local_exec {
        info!(
            "Running in executor with segment_limit_po2 = {:?}",
            segment_limit_po2
        );

        let input = to_vec(&input).expect("Could not serialize input!");
        info!(
            "Input size: {} words ( {} MB )",
            input.len(),
            input.len() * 4 / 1_000_000
        );

        let mut profiler = risc0_zkvm::Profiler::new(OP_DERIVE_PATH, OP_DERIVE_ELF).unwrap();

        info!("Running the executor...");
        let start_time = Instant::now();
        let session = {
            let mut builder = ExecutorEnv::builder();
            builder
                .session_limit(None)
                .segment_limit_po2(segment_limit_po2)
                .add_input(&input);

            if args.profile {
                builder.trace_callback(profiler.make_trace_callback());
            }

            let env = builder.build().unwrap();
            let mut exec = Executor::from_elf(env, OP_DERIVE_ELF).unwrap();

            let segment_dir = tempdir().unwrap();

            exec.run_with_callback(|segment| {
                Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
            })
            .unwrap()
        };
        info!(
            "Generated {:?} segments; elapsed time: {:?}",
            session.segments.len(),
            start_time.elapsed()
        );

        if args.profile {
            profiler.finalize();

            let sys_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            tokio::fs::write(
                format!("profile_{}.pb", sys_time.as_secs()),
                &profiler.encode_to_vec(),
            )
            .await
            .expect("Failed to write profiling output");
        }

        info!(
            "Executor ran in (roughly) {} cycles",
            session.segments.len() * (1 << segment_limit_po2)
        );

        // todo: validate journal output
    }

    // todo: bonsai stuff

    Ok(())
}

fn get_initial_zop_data(args: &Args) -> Result<DerivationInput> {
    let mut op_block_no = args.block_no;

    // Create dynamic block derivation struct
    println!("Fetch op head {}", op_block_no);
    let mut op_head_provider =
        new_provider(op_cache_path(&args, op_block_no), args.op_rpc_url.clone())?;
    let op_head = op_head_provider.get_full_block(&BlockQuery {
        block_no: op_block_no,
    })?;
    op_head_provider.save()?;
    // read system config from op_head (seq_no/epoch_no..etc)
    let system_tx_data = op_head.transactions.first().unwrap().input.to_vec();
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
    let eth_block_hash = decoded_data[3].clone().into_fixed_bytes().unwrap();
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

    println!("Fetch eth head {}", eth_block_no);
    let mut eth_head_provider = new_provider(
        eth_cache_path(&args, eth_block_no),
        args.eth_rpc_url.clone(),
    )?;
    let eth_head = eth_head_provider.get_partial_block(&BlockQuery {
        block_no: eth_block_no,
    })?;
    eth_head_provider.save()?;
    if eth_head.hash.unwrap().0.as_slice() != eth_block_hash.as_slice() {
        bail!("Ethereum head block hash mismatch.")
    }
    let op_state = RefCell::new(State {
        current_l1_block_number: eth_block_no,
        current_l1_block_hash: BlockHash::from(eth_head.hash.unwrap().0),
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
    let mut eth_block_inputs = vec![];
    let mut op_epoch_deposit_block_ptr = 0usize;
    let mut op_block_inputs = vec![];
    let target_block_no = args.block_no + args.blocks;
    while op_block_no < target_block_no {
        println!(
            "Process op block {} as of epoch {}",
            op_block_no, eth_block_no
        );
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
        op_epoch_queue.enqueue(epoch).unwrap();
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
        if receipts.is_some() {
            println!("Process config and batches");
            // update the system config
            op_system_config
                .update(&op_chain_config, &block_input)
                .context("failed to update system config")?;
            // process all batcher transactions
            BatcherTransactions::<1024, 1024>::process(
                op_chain_config.batch_inbox,
                op_system_config.batch_sender,
                block_input.block_header.number,
                &block_input.transactions,
                &op_buffer,
            )
            .context("failed to create batcher transactions")?;
        };

        eth_block_inputs.push(block_input);

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
                    op_block_seq_no += 1;
                    None
                }
            };
            deque_next_epoch_if_none(&op_state, &mut op_epoch_queue)?;
            // Process block transactions
            let mut op_state = op_state.borrow_mut();
            if op_batch.essence.parent_hash == op_state.safe_head.hash {
                op_block_no += 1;

                let eth_block_header = &eth_block_inputs[op_epoch_deposit_block_ptr].block_header;
                // run block builder with optimism strategy bundle
                let new_op_head = {
                    println!("Deriving op block");
                    // Fetch all of the initial data
                    let init = zeth_lib::host::get_initial_data::<OptimismStrategyBundle>(
                        OP_MAINNET_CHAIN_SPEC.clone(),
                        op_cache_path(&args, op_block_no),
                        args.op_rpc_url.clone(),
                        op_block_no,
                    )?;
                    let input: Input<OptimismTxEssence> = init.clone().into();

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
                        essence: OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
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
                        }),
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
                    let op_input_transactions: Vec<_> =
                        input.transactions.iter().map(|tx| tx.to_rlp()).collect();

                    if op_derived_transactions != op_input_transactions {
                        println!(
                            "{}/{}",
                            op_derived_transactions.len(),
                            op_input_transactions.len()
                        );
                        for (i, (derived, input)) in
                            zip(op_derived_transactions.iter(), op_input_transactions.iter())
                                .enumerate()
                        {
                            if derived != input {
                                let der: String =
                                    derived.iter().map(|n| format!("{:02x}", n)).collect();
                                let inp: String =
                                    input.iter().map(|n| format!("{:02x}", n)).collect();
                                println!("Mismatch at index {}\n{}\n{}", i, der, inp);
                            }
                        }
                        bail!("Derived transactions do not match provided input transactions!");
                    }

                    // derive
                    op_block_inputs.push(input.clone());
                    ConfiguredBlockBuilder::<OptimismStrategyBundle>::build_from(
                        &OP_MAINNET_CHAIN_SPEC,
                        input,
                    )?
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
            } else {
                println!("skipped batch w/ timestamp {}", op_batch.essence.timestamp);
            }
        }

        eth_provider.save()?;
        eth_block_no += 1;
    }
    Ok(DerivationInput {
        eth_block_inputs,
        op_block_inputs,
        op_head: BlockInput {
            block_header: op_head.clone().try_into().unwrap(),
            transactions: op_head
                .transactions
                .into_iter()
                .map(|tx| tx.try_into().unwrap())
                .collect(),
            receipts: None,
        },
    })
}
