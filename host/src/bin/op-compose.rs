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
    collections::VecDeque,
    fmt::Debug,
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Parser;
use log::{error, info};
use risc0_zkvm::{default_prover, serde::to_vec, Assumption, ExecutorEnv, Receipt};
use serde::{de::DeserializeOwned, Serialize};
use zeth_guests::*;
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::{
        batcher_db::{BatcherDb, BlockInput, MemDb},
        composition::{ComposeInput, ComposeInputOperation, ComposeOutputOperation},
        config::OPTIMISM_CHAIN_SPEC,
        DeriveInput, DeriveMachine,
    },
};
use zeth_primitives::{
    block::Header,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence},
    tree::MerkleMountainRange,
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
    block_no: u64,

    #[clap(long, require_equals = true)]
    /// Number of L2 blocks to provably derive.
    blocks: u64,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "20")]
    /// Runs the verification inside the zkvm executor locally. Accepts a custom maximum
    /// segment cycle count as a power of 2. [default: 20]
    local_exec: Option<u32>,
}

fn cache_file_path(cache_path: &Path, network: &str, block_no: u64, ext: &str) -> PathBuf {
    cache_path
        .join(network)
        .join(block_no.to_string())
        .with_extension(ext)
}

fn eth_cache_path(cache: &Option<PathBuf>, block_no: u64) -> Option<PathBuf> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "ethereum", block_no, "json.gz"))
}

fn op_cache_path(cache: &Option<PathBuf>, block_no: u64) -> Option<PathBuf> {
    cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "optimism", block_no, "json.gz"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    // OP Derivation
    info!("Fetching data ...");
    let mut lift_queue = Vec::new();
    let mut eth_chain: Vec<Header> = Vec::new();
    for i in 0..args.blocks {
        let db = RpcDb::new(
            args.eth_rpc_url.clone(),
            args.op_rpc_url.clone(),
            args.cache.clone(),
        );
        let local_exec = args.local_exec.clone();
        let (input, output, chain) = tokio::task::spawn_blocking(move || {
            let derive_input = DeriveInput {
                db,
                op_head_block_no: args.block_no + i,
                op_derive_block_count: 1,
            };
            let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
                .expect("Could not create derive machine");
            let eth_head_no = derive_machine.op_batcher.state.epoch.number;
            let eth_head = derive_machine
                .derive_input
                .db
                .get_eth_block_header(eth_head_no)
                .context("could not fetch eth head")?;
            let derive_output = derive_machine.derive().context("could not derive")?;
            let eth_tail = derive_machine
                .derive_input
                .db
                .get_eth_block_header(derive_output.eth_tail.0)
                .context("could not fetch eth tail")?;
            let mut eth_chain = vec![eth_head];
            for block_no in (eth_head_no + 1)..eth_tail.number {
                let eth_block = derive_machine
                    .derive_input
                    .db
                    .get_eth_block_header(block_no)
                    .context("could not fetch eth block")?;
                eth_chain.push(eth_block);
            }
            eth_chain.push(eth_tail);

            let derive_input_mem = DeriveInput {
                db: derive_machine.derive_input.db.get_mem_db(),
                op_head_block_no: args.block_no + i,
                op_derive_block_count: 1,
            };
            let out: anyhow::Result<_> = Ok((derive_input_mem, derive_output, eth_chain));
            out
        })
        .await??;

        info!("Running from memory ...");
        {
            let output_mem = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, input.clone())
                .expect("Could not create derive machine")
                .derive()
                .unwrap();
            assert_eq!(output, output_mem);
        }

        let receipt = maybe_prove(local_exec, &input, OP_DERIVE_ELF, &output, vec![]);

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
        eth_mountain_range.logged_append_leaf(block.hash().0, &mut sibling_map);
    }
    let eth_chain_root = eth_mountain_range
        .logged_root(&mut sibling_map)
        .expect("No eth blocks loaded!");
    let prep_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::PREP {
            eth_blocks: eth_chain,
            mountain_range: Default::default(),
            prior: None,
        },
        eth_chain_root,
    };
    let prep_compose_output = prep_compose_input.clone().process();

    let prep_compose_receipt = maybe_prove(
        args.local_exec.clone(),
        &prep_compose_input,
        OP_COMPOSE_ELF,
        &prep_compose_output,
        vec![],
    );

    // Lift
    let mut join_queue = VecDeque::new();
    for (derive_output, derive_receipt) in lift_queue {
        let eth_tail_hash = derive_output.eth_tail.1 .0;
        let lift_compose_input = ComposeInput {
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::LIFT {
                derivation: derive_output,
                eth_tail_proof: MerkleMountainRange::proof(&sibling_map, eth_tail_hash),
            },
            eth_chain_root,
        };
        let lift_compose_output = lift_compose_input.clone().process();

        let lift_compose_receipt = if let Some(receipt) = derive_receipt {
            maybe_prove(
                args.local_exec.clone(),
                &lift_compose_input,
                OP_COMPOSE_ELF,
                &lift_compose_output,
                vec![receipt.into()],
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
            eth_chain_root,
        };
        let join_compose_output = join_compose_input.clone().process();

        let join_compose_receipt =
            if let (Some(left_receipt), Some(right_receipt)) = (left_receipt, right_receipt) {
                maybe_prove(
                    args.local_exec.clone(),
                    &join_compose_input,
                    OP_COMPOSE_ELF,
                    &join_compose_output,
                    vec![left_receipt.into(), right_receipt.into()],
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
        eth_chain_root,
    };
    let finish_compose_output = finish_compose_input.clone().process();

    let op_compose_receipt = if let (Some(prep_receipt), Some(aggregate_receipt)) =
        (prep_compose_receipt, aggregate_receipt)
    {
        maybe_prove(
            args.local_exec.clone(),
            &finish_compose_input,
            OP_COMPOSE_ELF,
            &finish_compose_output,
            vec![prep_receipt.into(), aggregate_receipt.into()],
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
) -> Option<Receipt> {
    if let Some(segment_limit_po2) = local_exec {
        let encoded_input = to_vec(input).expect("Could not serialize composition prep input!");
        let receipt = prove(segment_limit_po2, encoded_input, elf, assumptions);
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
) -> Receipt {
    info!(
        "Proving derivation with segment_limit_po2 = {:?}",
        segment_limit_po2
    );
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

    for assumption in assumptions {
        env_builder.add_assumption(assumption);
    }

    let prover = default_prover();
    prover.prove(env_builder.build().unwrap(), elf).unwrap()
}

#[derive(Clone)]

pub struct RpcDb {
    eth_rpc_url: Option<String>,
    op_rpc_url: Option<String>,
    cache: Option<PathBuf>,
    mem_db: MemDb,
}

impl RpcDb {
    pub fn new(
        eth_rpc_url: Option<String>,
        op_rpc_url: Option<String>,
        cache: Option<PathBuf>,
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
    fn get_full_op_block(
        &mut self,
        block_no: u64,
    ) -> anyhow::Result<BlockInput<OptimismTxEssence>> {
        let mut provider = new_provider(
            op_cache_path(&self.cache, block_no),
            self.op_rpc_url.clone(),
        )?;
        let block = {
            let ethers_block = provider.get_full_block(&BlockQuery { block_no })?;
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
        self.mem_db.full_op_block.insert(block_no, block.clone());
        provider.save()?;
        Ok(block)
    }

    fn get_op_block_header(&mut self, block_no: u64) -> anyhow::Result<Header> {
        let mut provider = new_provider(
            op_cache_path(&self.cache, block_no),
            self.op_rpc_url.clone(),
        )?;
        let header: Header = provider
            .get_partial_block(&BlockQuery { block_no })?
            .try_into()?;
        self.mem_db.op_block_header.insert(block_no, header.clone());
        provider.save()?;
        Ok(header)
    }

    fn get_full_eth_block(
        &mut self,
        block_no: u64,
    ) -> anyhow::Result<BlockInput<EthereumTxEssence>> {
        let query = BlockQuery { block_no };
        let mut provider = new_provider(
            eth_cache_path(&self.cache, block_no),
            self.eth_rpc_url.clone(),
        )?;
        let block = {
            let ethers_block = provider.get_full_block(&query)?;
            let block_header: Header = ethers_block.clone().try_into().unwrap();
            // include receipts when needed
            let can_contain_deposits = zeth_lib::optimism::deposits::can_contain(
                &OPTIMISM_CHAIN_SPEC.deposit_contract,
                &block_header.logs_bloom,
            );
            let can_contain_config = zeth_lib::optimism::system_config::can_contain(
                &OPTIMISM_CHAIN_SPEC.system_config_contract,
                &block_header.logs_bloom,
            );
            let receipts = if can_contain_config || can_contain_deposits {
                let receipts = provider.get_block_receipts(&query)?;
                Some(
                    receipts
                        .into_iter()
                        .map(|receipt| receipt.try_into())
                        .collect::<anyhow::Result<Vec<_>, _>>()
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
        self.mem_db.full_eth_block.insert(block_no, block.clone());
        provider.save()?;
        Ok(block)
    }

    fn get_eth_block_header(&mut self, block_no: u64) -> anyhow::Result<Header> {
        let mut provider = new_provider(
            eth_cache_path(&self.cache, block_no),
            self.eth_rpc_url.clone(),
        )?;
        let header: Header = provider
            .get_partial_block(&BlockQuery { block_no })?
            .try_into()?;
        self.mem_db
            .eth_block_header
            .insert(block_no, header.clone());
        provider.save()?;
        Ok(header)
    }
}
