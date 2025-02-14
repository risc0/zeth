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

use alloy::primitives::{keccak256, U256};
use alloy_chains::NamedChain;
use clap::Parser;
use std::process::Command;
use tracing::{error, info};
use zeth::cli::ProveArgs;

#[derive(clap::Parser, Debug, Clone)]
#[command(name = "zeth-benchmark")]
#[command(bin_name = "zeth-benchmark")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(flatten)]
    pub prove_args: ProveArgs,

    #[clap(
        long,
        require_equals = true,
        value_enum,
        conflicts_with = "chain",
        required_unless_present = "chain"
    )]
    /// Which chain spec to use.
    pub chain_id: Option<NamedChain>,

    #[clap(long, require_equals = true)]
    /// The range of blocks after the starting block number to sample from
    pub sample_range: u64,

    #[clap(long, require_equals = true)]
    /// The number of samples to benchmark
    pub sample_count: u64,

    /// Path to the zeth program used for proving
    #[clap(long, require_equals = true)]
    pub zeth: Option<String>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();
    let build_args = &cli.prove_args.run_args.build_args;
    let chain_id = build_args.chain.or(cli.chain_id).unwrap();
    // generate sequence of starting block numbers to benchmark
    let seed = keccak256(
        [
            (chain_id as u64).to_be_bytes(),
            build_args.block_number.to_be_bytes(),
            build_args.block_count.to_be_bytes(),
            cli.sample_range.to_be_bytes(),
            cli.sample_count.to_be_bytes(),
        ]
        .concat(),
    );
    let block_numbers = (0..cli.sample_count)
        .map(|i| {
            build_args.block_number
                + U256::from_be_bytes(
                    keccak256([seed.as_slice(), i.to_be_bytes().as_slice()].concat()).0,
                )
                .reduce_mod(U256::from(cli.sample_range))
                .to::<u64>()
        })
        .collect::<Vec<_>>();
    // Report samples
    info!(
        "Printing {} sample block number(s) to stdout.",
        block_numbers.len()
    );
    for n in &block_numbers {
        println!("{n}");
    }
    // run zeth
    if let Some(program) = cli.zeth {
        info!("Executing {program} to prove each sample");
        let run_args = &cli.prove_args.run_args;
        for block_number in block_numbers {
            let mut command = Command::new(&program);
            command.arg("prove");
            // build args
            if let Some(rpc) = &build_args.rpc {
                command.arg(format!("--rpc={rpc}"));
            }
            if let Some(cache) = &build_args.cache {
                command.arg(format!("--cache={}", cache.display()));
            }
            command.arg(format!("--block-number={block_number}"));
            command.arg(format!("--block-count={}", build_args.block_count));
            command.arg(format!("--chain={chain_id}"));
            // run args
            command.arg(format!("--execution-po2={}", run_args.execution_po2));
            if run_args.profile {
                command.arg("--profile");
            }
            // prove args
            if cli.prove_args.snark {
                command.arg("--snark");
            }
            match command.status() {
                Ok(exit_code) => {
                    if exit_code.success() {
                        info!("zeth terminated successfully for block {block_number}");
                    } else {
                        error!(
                            "zeth terminated with exit code {exit_code} for block {block_number}"
                        );
                    }
                }
                Err(err) => {
                    error!("Error executing zeth for block {block_number}: {:?}", err);
                }
            }
        }
    }
}
