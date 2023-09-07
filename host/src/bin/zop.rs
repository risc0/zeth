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

use anyhow::Context;
use clap::Parser;
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::{derivation::CHAIN_SPEC, epoch::BlockInput},
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let mut eth_block_no = args.epoch_no;
    let mut eth_blocks = vec![];
    let mut op_block_no = args.block_no;
    // let mut op_inputs: vec![];

    while op_block_no < args.block_no + args.blocks {
        let eth_rpc_cache = args
            .cache
            .as_ref()
            .map(|dir| cache_file_path(dir, "ethereum", eth_block_no, "json.gz"));

        let mut eth_provider = new_provider(eth_rpc_cache, args.eth_rpc_url.clone())?;

        // get the block header
        let block_query = BlockQuery {
            block_no: eth_block_no,
        };
        let eth_block = eth_provider
            .get_full_block(&block_query)
            .context("block not found")?;
        let header: Header = eth_block
            .clone()
            .try_into()
            .context("invalid block header")?;

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

        if let Some(ref receipts) = receipts {
            // todo: derive batches from eth block
        };

        eth_blocks.push(BlockInput {
            block_header: header,
            receipts: receipts.clone(),
            transactions: eth_block
                .transactions
                .into_iter()
                .map(|tx| tx.try_into().unwrap())
                .collect(),
        });
        eth_block_no += 1;
    }

    for i in 0..args.blocks {
        let l2_block_no = args.block_no + i;

        let l2_rpc_cache = args
            .cache
            .as_ref()
            .map(|dir| cache_file_path(dir, "optimism", l2_block_no, "json.gz"));
    }

    Ok(())
}
