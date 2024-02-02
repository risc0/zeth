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

// Example usage:
//
// RISC0_DEV_MODE=true RUST_LOG=info RUST_BACKTRACE=full ./target/release/op-compose \
// --eth-rpc-url="https://eth-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
// --op-rpc-url="https://opt-mainnet.g.alchemy.com/v2/API_KEY_HERE" \
// --cache \
// --op-block-no=112875552 \
// --op-blocks=8 \
// --op-blocks-step=2 \

use std::{collections::VecDeque, fmt::Debug, path::PathBuf};

use anyhow::Context;
use clap::Parser;
use log::{error, info};
use risc0_zkvm::{default_prover, serde::to_vec, Assumption, ExecutorEnv, Receipt};
use serde::{de::DeserializeOwned, Serialize};
use zeth_guests::*;
use zeth_lib::{
    host::rpc_db::RpcDb,
    optimism::{
        batcher_db::BatcherDb,
        composition::{ComposeInput, ComposeInputOperation, ComposeOutputOperation},
        config::OPTIMISM_CHAIN_SPEC,
        DeriveInput, DeriveMachine,
    },
};
use zeth_primitives::{
    block::Header,
    mmr::{MerkleMountainRange, MerkleProof},
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
    cache: Option<PathBuf>,

    #[clap(long, require_equals = true)]
    /// L2 block number to begin from
    op_block_no: u64,

    #[clap(long, require_equals = true)]
    /// Number of L2 blocks to provably derive.
    op_blocks: u64,

    #[clap(long, require_equals = true, default_value = "1")]
    /// Number of L2 blocks to process per derivation call.
    op_blocks_step: u64,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "20")]
    /// Runs the verification inside the zkvm executor locally. Accepts a custom maximum
    /// segment cycle count as a power of 2. [default: 20]
    local_exec: Option<u32>,

    #[clap(short, long, default_value_t = false)]
    /// Whether to profile the zkVM execution
    profile: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    // OP Derivation
    info!("Fetching data ...");
    let mut lift_queue = Vec::new();
    let mut eth_chain: Vec<Header> = Vec::new();
    for op_block_index in (0..args.op_blocks).step_by(args.op_blocks_step as usize) {
        let db = RpcDb::new(
            args.eth_rpc_url.clone(),
            args.op_rpc_url.clone(),
            args.cache.clone(),
        );
        let local_exec = args.local_exec;
        let (input, output, chain) = tokio::task::spawn_blocking(move || {
            let derive_input = DeriveInput {
                db,
                op_head_block_no: args.op_block_no + op_block_index,
                op_derive_block_count: args.op_blocks_step,
            };
            let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
                .expect("Could not create derive machine");
            let eth_head_no = derive_machine.op_batcher.state.epoch.number;
            let eth_head = derive_machine
                .derive_input
                .db
                .get_full_eth_block(eth_head_no)
                .context("could not fetch eth head")?
                .block_header
                .clone();
            let derive_output = derive_machine.derive().context("could not derive")?;
            let eth_tail = derive_machine
                .derive_input
                .db
                .get_full_eth_block(derive_output.eth_tail.number)
                .context("could not fetch eth tail")?
                .block_header
                .clone();
            let mut eth_chain = vec![eth_head];
            for block_no in (eth_head_no + 1)..eth_tail.number {
                eth_chain.push(
                    derive_machine
                        .derive_input
                        .db
                        .get_full_eth_block(block_no)
                        .context("could not fetch eth block")?
                        .block_header
                        .clone(),
                );
            }
            eth_chain.push(eth_tail);

            let derive_input_mem = DeriveInput {
                db: derive_machine.derive_input.db.get_mem_db(),
                op_head_block_no: args.op_block_no + op_block_index,
                op_derive_block_count: args.op_blocks_step,
            };
            let out: anyhow::Result<_> = Ok((derive_input_mem, derive_output, eth_chain));
            out
        })
        .await??;

        info!("Deriving ...");
        {
            let output_mem = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, input.clone())
                .expect("Could not create derive machine")
                .derive()
                .unwrap();
            assert_eq!(output, output_mem);
        }

        let receipt = maybe_prove(
            local_exec,
            &input,
            OP_DERIVE_ELF,
            &output,
            vec![],
            args.profile,
        );

        // Append derivation outputs to lift queue
        lift_queue.push((output, receipt));
        // Extend block chain
        for block in chain {
            let tail_num = match eth_chain.last() {
                None => 0u64,
                Some(tail) => tail.number,
            };
            // This check should be sufficient
            if tail_num < block.number {
                eth_chain.push(block);
            }
        }
    }

    // OP Composition
    // Prep
    let mut sibling_map = Default::default();
    let mut eth_mountain_range: MerkleMountainRange = Default::default();
    for block in &eth_chain {
        eth_mountain_range.append_leaf(block.hash().0, Some(&mut sibling_map));
    }
    let eth_chain_root = eth_mountain_range
        .root(Some(&mut sibling_map))
        .expect("No eth blocks loaded!");
    let prep_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::PREP {
            eth_blocks: eth_chain,
            prior_prep: None,
        },
        eth_chain_merkle_root: eth_chain_root,
    };
    info!("Preparing ...");
    let prep_compose_output = prep_compose_input
        .clone()
        .process()
        .expect("Prep composition failed.");

    let prep_compose_receipt = maybe_prove(
        args.local_exec,
        &prep_compose_input,
        OP_COMPOSE_ELF,
        &prep_compose_output,
        vec![],
        args.profile,
    );

    // Lift
    let mut join_queue = VecDeque::new();
    for (derive_output, derive_receipt) in lift_queue {
        let eth_tail_hash = derive_output.eth_tail.hash.0;
        let lift_compose_input = ComposeInput {
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::LIFT {
                derivation: derive_output,
                eth_tail_proof: MerkleProof::new(&sibling_map, eth_tail_hash),
            },
            eth_chain_merkle_root: eth_chain_root,
        };
        info!("Lifting ...");
        let lift_compose_output = lift_compose_input
            .clone()
            .process()
            .expect("Lift composition failed.");

        let lift_compose_receipt = if let Some(receipt) = derive_receipt {
            maybe_prove(
                args.local_exec,
                &lift_compose_input,
                OP_COMPOSE_ELF,
                &lift_compose_output,
                vec![receipt.into()],
                args.profile,
            )
        } else {
            None
        };

        join_queue.push_back((lift_compose_output, lift_compose_receipt));
    }

    // Join
    while join_queue.len() > 1 {
        let (left, left_receipt) = join_queue.pop_front().unwrap();
        let (right, _right_receipt) = join_queue.front().unwrap();
        let ComposeOutputOperation::AGGREGATE {
            op_tail: left_op_tail,
            ..
        } = &left.operation
        else {
            panic!("Expected left aggregate operation output!")
        };
        let ComposeOutputOperation::AGGREGATE {
            op_head: right_op_head,
            ..
        } = &right.operation
        else {
            panic!("Expected right aggregate operation output!")
        };
        // Push dangling workloads (odd block count) to next round
        if left_op_tail != right_op_head {
            join_queue.push_back((left, left_receipt));
            continue;
        }
        // Pair up join
        let (right, right_receipt) = join_queue.pop_front().unwrap();
        let join_compose_input = ComposeInput {
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::JOIN { left, right },
            eth_chain_merkle_root: eth_chain_root,
        };
        info!("Joining ...");
        let join_compose_output = join_compose_input
            .clone()
            .process()
            .expect("Join composition failed.");

        let join_compose_receipt =
            if let (Some(left_receipt), Some(right_receipt)) = (left_receipt, right_receipt) {
                maybe_prove(
                    args.local_exec,
                    &join_compose_input,
                    OP_COMPOSE_ELF,
                    &join_compose_output,
                    vec![left_receipt.into(), right_receipt.into()],
                    args.profile,
                )
            } else {
                None
            };

        // Send workload to next round
        join_queue.push_back((join_compose_output, join_compose_receipt));
    }

    // Finish
    let (aggregate_output, aggregate_receipt) = join_queue.pop_front().unwrap();
    let finish_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::FINISH {
            prep: prep_compose_output,
            aggregate: aggregate_output,
        },
        eth_chain_merkle_root: eth_chain_root,
    };
    info!("Finishing ...");
    let finish_compose_output = finish_compose_input
        .clone()
        .process()
        .expect("Finish composition failed.");

    let op_compose_receipt = if let (Some(prep_receipt), Some(aggregate_receipt)) =
        (prep_compose_receipt, aggregate_receipt)
    {
        maybe_prove(
            args.local_exec,
            &finish_compose_input,
            OP_COMPOSE_ELF,
            &finish_compose_output,
            vec![prep_receipt.into(), aggregate_receipt.into()],
            args.profile,
        )
    } else {
        None
    };

    dbg!(&finish_compose_output);

    if let Some(final_receipt) = op_compose_receipt {
        final_receipt
            .verify(OP_COMPOSE_ID)
            .expect("Failed to verify final receipt");
        info!("Verified final receipt!");
    }

    Ok(())
}

pub fn maybe_prove<I: Serialize, O: Eq + Debug + DeserializeOwned>(
    local_exec: Option<u32>,
    input: &I,
    elf: &[u8],
    expected_output: &O,
    assumptions: Vec<Assumption>,
    profile: bool,
) -> Option<Receipt> {
    if let Some(segment_limit_po2) = local_exec {
        let encoded_input = to_vec(input).expect("Could not serialize composition prep input!");
        let receipt = prove(segment_limit_po2, encoded_input, elf, assumptions, profile);
        let output_guest: O = receipt.journal.decode().unwrap();

        if expected_output == &output_guest {
            info!("Executor succeeded");
        } else {
            error!(
                "Output mismatch! Executor: {:?}, expected: {:?}",
                output_guest, expected_output,
            );
        }
        Some(receipt)
    } else {
        None
    }
}

pub fn prove(
    segment_limit_po2: u32,
    encoded_input: Vec<u32>,
    elf: &[u8],
    assumptions: Vec<Assumption>,
    profile: bool,
) -> Receipt {
    info!("Proving with segment_limit_po2 = {:?}", segment_limit_po2);
    info!(
        "Input size: {} words ( {} MB )",
        encoded_input.len(),
        encoded_input.len() * 4 / 1_000_000
    );

    info!("Running the prover...");
    let mut env_builder = ExecutorEnv::builder();

    env_builder
        .session_limit(None)
        .segment_limit_po2(segment_limit_po2)
        .write_slice(&encoded_input);

    if profile {
        info!("Profiling enabled.");
        let sys_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();

        env_builder.enable_profiler(format!("profile_opc_{}.pb", sys_time.as_nanos()));
    }

    for assumption in assumptions {
        env_builder.add_assumption(assumption);
    }

    let prover = default_prover();
    prover.prove(env_builder.build().unwrap(), elf).unwrap()
}
