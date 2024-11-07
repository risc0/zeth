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

use clap::ValueEnum;
use reth_chainspec::NamedChain;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(clap::Parser, Debug, Clone)]
#[command(name = "zeth")]
#[command(bin_name = "zeth")]
#[command(author, version, about, long_about = None)]
pub enum Cli {
    /// Build blocks natively outside the RISC Zero zkVM
    Build(BuildArgs),
    /// Build blocks inside the RISC Zero zkVM executor
    Run(RunArgs),
    /// Provably build blocks inside the RISC Zero zkVM
    Prove(ProveArgs),
    /// Verify a block building proof
    Verify(VerifyArgs),
}

impl Cli {
    pub fn build_args(&self) -> &BuildArgs {
        match &self {
            Cli::Build(args) => args,
            Cli::Run(args) => &args.build_args,
            Cli::Prove(args) => &args.run_args.build_args,
            Cli::Verify(args) => &args.build_args,
        }
    }

    pub fn run_args(&self) -> &RunArgs {
        match &self {
            Cli::Run(args) => args,
            Cli::Prove(args) => &args.run_args,
            _ => unreachable!(),
        }
    }

    pub fn prove_args(&self) -> &ProveArgs {
        match &self {
            Cli::Prove(prove_args) => prove_args,
            _ => unreachable!(),
        }
    }

    pub fn verify_args(&self) -> &VerifyArgs {
        match &self {
            Cli::Verify(verify_args) => verify_args,
            _ => unreachable!(),
        }
    }

    pub fn should_build(&self) -> bool {
        !matches!(self, Cli::Verify(..))
    }

    pub fn should_execute(&self) -> bool {
        !matches!(self, Cli::Build(..) | Cli::Verify(..))
    }

    pub fn should_prove(&self) -> bool {
        matches!(self, Cli::Prove(..))
    }

    pub fn snark(&self) -> bool {
        if let Cli::Prove(prove_args) = self {
            prove_args.snark
        } else {
            false
        }
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct BuildArgs {
    #[clap(short = 'u', long, require_equals = true)]
    /// URL of the execution-layer RPC node
    pub rpc: Option<String>,

    #[clap(short = 'd', long, require_equals = true, num_args = 0..=1, default_missing_value = "cache_rpc")]
    /// Directory for caching RPC data; the value specifies the cache directory
    ///
    /// [default when the flag is present: cache_rpc]
    pub cache: Option<PathBuf>,

    #[clap(short = 'b', long, require_equals = true)]
    /// Starting block number
    pub block_number: u64,

    #[clap(short = 'n', long, require_equals = true, default_value_t = 1)]
    /// Number of blocks to build in a single proof
    pub block_count: u64,

    #[clap(short = 'c', long, require_equals = true, value_enum)]
    /// Which chain spec to use.
    pub chain: Option<NamedChain>,
}

#[derive(Debug, Clone, clap::ValueEnum, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum Chain {
    /// Mainnet
    Mainnet,
    /// Sepolia testnet
    Sepolia,
    /// Holesky testnet
    Holesky,
    /// Devnet
    Dev,
}

impl Display for Chain {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // use the name of the clap::ValueEnum
        let val = self.to_possible_value().unwrap();
        write!(f, "{}", val.get_name())
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    #[clap(flatten)]
    pub build_args: BuildArgs,

    #[clap(short = 'e', long, require_equals = true, default_value_t = 20)]
    /// The maximum cycle count of a segment as a power of 2
    pub execution_po2: u32,

    #[clap(short = 'p', long, default_value_t = false)]
    /// Save the profile of the execution in the current working directly
    pub profile: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct ProveArgs {
    #[clap(flatten)]
    pub run_args: RunArgs,

    #[clap(short = 's', long, default_value_t = false)]
    /// Convert the resulting STARK receipt into a Groth-16 SNARK
    pub snark: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyArgs {
    #[clap(flatten)]
    pub build_args: BuildArgs,

    #[clap(short = 'f', long, require_equals = true)]
    /// Receipt file path
    pub file: PathBuf,
}

#[derive(clap::Args, Debug, Clone)]
pub struct BenchArgs {
    #[clap(flatten)]
    pub prove_args: ProveArgs,

    #[clap(short = 'r', long, require_equals = true)]
    /// The number of blocks to sample from
    pub sample_range: u64,

    #[clap(short = 'm', long, require_equals = true)]
    /// The number of samples to benchmark
    pub sample_count: u64,
}
