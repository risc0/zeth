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

use std::path::{Path, PathBuf};

use alloy_sol_types::SolInterface;
use anyhow::Result;
use clap::Parser;
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::OpSystemInfo,
};

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, require_equals = true)]
    /// URL of the L2 RPC node.
    op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata]
    cache: Option<PathBuf>,

    #[clap(long, require_equals = true)]
    /// L2 block number to query
    block_no: u64,
}

fn cache_file_path(cache_path: &Path, network: &str, block_no: u64, ext: &str) -> PathBuf {
    cache_path
        .join(network)
        .join(block_no.to_string())
        .with_extension(ext)
}

fn op_cache_path(args: &Args, block_no: u64) -> Option<PathBuf> {
    args.cache
        .as_ref()
        .map(|dir| cache_file_path(dir, "optimism", block_no, "json.gz"))
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let op_block = tokio::task::spawn_blocking(move || {
        let mut provider =
            new_provider(op_cache_path(&args, args.block_no), args.op_rpc_url.clone())
                .expect("Could not create provider");

        let op_block = provider
            .get_full_block(&BlockQuery {
                block_no: args.block_no,
            })
            .expect("Could not fetch OP block");
        provider.save().expect("Could not save cache");

        op_block
    })
    .await?;

    let system_tx_data = op_block
        .transactions
        .first()
        .expect("No transactions")
        .input
        .to_vec();
    let set_l1_block_values = OpSystemInfo::OpSystemInfoCalls::abi_decode(&system_tx_data, true)
        .expect("Could not decode call data");

    println!("{:?}", set_l1_block_values);

    Ok(())
}
