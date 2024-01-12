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

use std::path::PathBuf;

use zeth_lib::consts::Network;

#[derive(clap::Parser, Debug, Clone)] // requires `derive` feature
#[command(name = "zeth")]
#[command(bin_name = "zeth")]
#[command(author, version, about, long_about = None)]
pub enum Cli {
    /// Build blocks natively outside the zkVM
    Build(BuildArgs),
    /// Run the block creation process inside the executor
    Run(RunArgs),
    /// Provably create blocks inside the zkVM
    Prove(ProveArgs),
    /// Verify a block creation receipt
    Verify(VerifyArgs),
    /// Output debug information about an optimism block
    OpInfo(CoreArgs),
}

impl Cli {
    pub fn core_args(&self) -> &CoreArgs {
        match &self {
            Cli::Build(build_args) => &build_args.core_args,
            Cli::Run(run_args) => &run_args.core_args,
            Cli::Prove(prove_args) => &prove_args.core_args,
            Cli::Verify(verify_args) => &verify_args.core_args,
            Cli::OpInfo(core_args) => core_args,
        }
    }

    pub fn composition(&self) -> Option<u64> {
        match &self {
            Cli::Build(build_args) => build_args.composition_args.composition,
            Cli::Prove(prove_args) => prove_args.composition_args.composition,
            _ => None,
        }
    }
}

impl ToString for Cli {
    fn to_string(&self) -> String {
        match self {
            Cli::Build(BuildArgs {
                core_args,
                composition_args,
            }) => format!(
                "build_{}_{}_{}_{}",
                core_args.network.to_string(),
                core_args.block_number,
                core_args.block_count,
                composition_args.composition.unwrap_or(0)
            ),
            Cli::Run(RunArgs { core_args, .. }) => format!(
                "run_{}_{}_{}",
                core_args.network.to_string(),
                core_args.block_number,
                core_args.block_count,
            ),
            Cli::Prove(ProveArgs {
                core_args,
                composition_args,
                ..
            }) => format!(
                "prove_{}_{}_{}_{}",
                core_args.network.to_string(),
                core_args.block_number,
                core_args.block_count,
                composition_args.composition.unwrap_or(0)
            ),
            Cli::Verify(VerifyArgs { core_args, .. }) => format!(
                "verify_{}_{}_{}",
                core_args.network.to_string(),
                core_args.block_number,
                core_args.block_count
            ),
            Cli::OpInfo(core_args) => format!(
                "opinfo_{}_{}_{}",
                core_args.network.to_string(),
                core_args.block_number,
                core_args.block_count
            ),
        }
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct CoreArgs {
    #[clap(
        short = 'w',
        long,
        require_equals = true,
        value_enum,
        default_value = "ethereum"
    )]
    /// Network name (ethereum/optimism/optimism-derived).
    pub network: Network,

    #[clap(short, long, require_equals = true)]
    /// URL of the Ethereum RPC node.
    pub eth_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true)]
    /// URL of the Optimism RPC node.
    pub op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "host/testdata")]
    /// Use a local directory as a cache for RPC calls. Accepts a custom directory.
    /// [default: host/testdata]
    pub cache: Option<PathBuf>,

    #[clap(short, long, require_equals = true)]
    /// Block number to begin from
    pub block_number: u64,

    #[clap(short = 'n', long, require_equals = true, default_value_t = 1)]
    /// Number of blocks to provably derive.
    pub block_count: u64,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ExecutorArgs {
    #[clap(short, long, require_equals = true, default_value_t = 20)]
    /// The maximum segment cycle count as a power of 2.
    pub local_exec: u32,

    #[clap(short, long, default_value_t = false)]
    /// Whether to profile the zkVM execution
    pub profile: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CompositionArgs {
    #[clap(short='m', long, require_equals = true, num_args = 0..=1, default_missing_value = "1")]
    /// Compose separate block derivation proofs together. Accepts a custom number of
    /// blocks to process per derivation call. (optimism-derived network only)
    /// [default: 1]
    pub composition: Option<u64>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct BuildArgs {
    #[clap(flatten)]
    pub core_args: CoreArgs,
    #[clap(flatten)]
    pub composition_args: CompositionArgs,
}

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    #[clap(flatten)]
    pub core_args: CoreArgs,
    #[clap(flatten)]
    pub exec_args: ExecutorArgs,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ProveArgs {
    #[clap(flatten)]
    pub core_args: CoreArgs,
    #[clap(flatten)]
    pub exec_args: ExecutorArgs,
    #[clap(flatten)]
    pub composition_args: CompositionArgs,
    #[clap(short, long, default_value_t = false)]
    /// Prove remotely using Bonsai.
    pub submit_to_bonsai: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyArgs {
    #[clap(flatten)]
    pub core_args: CoreArgs,
    #[clap(short, long, require_equals = true)]
    /// Verify the receipt from the provided Bonsai Session UUID.
    pub receipt_bonsai_uuid: Option<String>,
}
