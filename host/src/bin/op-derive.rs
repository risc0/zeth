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

use anyhow::{Context, Result};
use clap::Parser;
use zeth_lib::{
    host::provider::{new_provider, BlockQuery},
    optimism::{derivation::CHAIN_SPEC, derive, epoch::BlockInput, BatcherDb, MemDb},
};
use zeth_primitives::{
    block::Header,
    transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence},
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

    let (mut mem_db, output_1) = tokio::task::spawn_blocking(move || {
        let mut rpc_db = RpcDb::new(args.eth_rpc_url, args.op_rpc_url, args.cache);
        let batches = derive(&mut rpc_db, args.block_no, args.blocks).unwrap();
        (rpc_db.get_mem_db(), batches)
    })
    .await?;

    let output_2 = derive(&mut mem_db, args.block_no, args.blocks).unwrap();
    assert_eq!(output_1, output_2);

    println!("Head: {}", output_1.head_block_hash);
    for derived_hash in output_1.derived_blocks {
        println!("Derived: {}", derived_hash);
    }

    Ok(())
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
    fn get_full_op_block(&mut self, block_no: u64) -> Result<BlockInput<OptimismTxEssence>> {
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

    fn get_op_block_header(&mut self, block_no: u64) -> Result<Header> {
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

    fn get_full_eth_block(&mut self, block_no: u64) -> Result<BlockInput<EthereumTxEssence>> {
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
                &CHAIN_SPEC.deposit_contract,
                &block_header.logs_bloom,
            );
            let can_contain_config = zeth_lib::optimism::system_config::can_contain(
                &CHAIN_SPEC.system_config_contract,
                &block_header.logs_bloom,
            );
            let receipts = if can_contain_config || can_contain_deposits {
                let receipts = provider.get_block_receipts(&query)?;
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
        self.mem_db.full_eth_block.insert(block_no, block.clone());
        provider.save()?;
        Ok(block)
    }

    fn get_eth_block_header(&mut self, block_no: u64) -> Result<Header> {
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
