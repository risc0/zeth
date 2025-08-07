// Copyright 2025 RISC Zero, Inc.
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

use alloy::{
    eips::BlockId,
    primitives::B256,
    providers::{Provider, ProviderBuilder},
};
use anyhow::{Context, ensure};
use clap::{Parser, Subcommand};
use reth_stateless::StatelessInput;
use std::{
    cmp::PartialEq,
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    sync::Arc,
};
use zeth_host::{BlockProcessor, to_zkvm_input_bytes};

/// Simple CLI to create Ethereum block execution proofs.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// URL of the Ethereum RPC endpoint to connect to.
    #[arg(long, env)]
    eth_rpc_url: String,

    /// Block number, tag, or hash (e.g., "latest", "0x1565483") to execute.
    #[arg(long, global = true, default_value = "latest")]
    block: BlockId,

    /// Cache folder for input files.
    #[arg(long, global = true, default_value = "./cache")]
    cache_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
enum Commands {
    /// Validate the block and generate a RISC Zero proof.
    Prove,
    /// Validate the block on the host machine, without proving.
    Validate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ensure the cache directory exists
    fs::create_dir_all(&cli.cache_dir).context("failed to create cache directory")?;

    // set up the provider and processor
    let provider = ProviderBuilder::new().connect(&cli.eth_rpc_url).await?;
    let processor = BlockProcessor::new(Arc::new(provider)).await?;
    println!("Current chain: {}", processor.chain());

    let input = get_cached_input(&processor, cli.block, &cli.cache_dir).await?;
    let block_hash = input.block.hash_slow();

    println!(
        "Input for block {} ({}): {:.3} MB",
        input.block.number,
        block_hash,
        to_zkvm_input_bytes(&input)?.len() as f64 / 1e6
    );

    // always validate
    processor.validate(input.clone()).context("host validation failed")?;
    println!("Host validation successful");

    // create proof if requested
    if cli.command == Commands::Prove {
        let (receipt, image_id) = processor.prove(input).await.context("proving failed")?;
        receipt.verify(image_id).context("proof verification failed")?;

        let proven_hash =
            B256::try_from(receipt.journal.as_ref()).context("failed to decode journal")?;
        ensure!(proven_hash == block_hash, "journal output mismatch");
    }

    Ok(())
}

async fn get_cached_input<P: Provider>(
    processor: &BlockProcessor<P>,
    block_id: BlockId,
    cache_dir: &Path,
) -> anyhow::Result<StatelessInput> {
    let block_hash = match block_id {
        BlockId::Hash(hash) => hash.block_hash,
        _ => {
            // First, get the block header to determine the canonical hash for caching.
            let header = processor
                .provider()
                .get_block(block_id)
                .await?
                .with_context(|| format!("block {block_id} not found"))?
                .header;

            header.hash
        }
    };

    let cache_file = cache_dir.join(format!("input_{block_hash}.json"));
    let input: StatelessInput = if cache_file.exists() {
        println!("Cache hit for block {block_hash}. Loading from file: {cache_file:?}");
        let f = File::open(&cache_file).context("failed to open file")?;
        serde_json::from_reader(BufReader::new(f)).context("failed to read file")?
    } else {
        println!("Cache miss for block {block_hash}. Fetching from RPC.");
        let (input, _) = processor.create_input(block_hash).await?;

        // Save the newly fetched input to the cache.
        println!("Writing new input to cache: {cache_file:?}");
        let f = File::create(&cache_file).context("failed to create file")?;
        serde_json::to_writer(BufWriter::new(f), &input).context("failed to write file")?;

        input
    };
    ensure!(input.block.hash_slow() == block_hash);

    Ok(input)
}
