// Copyright 2023, 2024 RISC Zero, Inc.
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

use clap::{Parser, Args, ValueEnum, Command};
use reth_chainspec::NamedChain;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
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

#[derive(Debug, Clone, Args)]
pub struct BuildArgs {
    #[arg(short = 'u', long, require_equals = true)]
    /// URL of the execution-layer RPC node
    pub rpc: Option<String>,

    #[arg(short = 'd', long, require_equals = true, num_args = 0..=1, default_missing_value = "cache_rpc")]
    /// Directory for caching RPC data; the value specifies the cache directory
    ///
    /// [default when the flag is present: cache_rpc]
    pub cache: Option<PathBuf>,

    #[arg(short = 'b', long, require_equals = true)]
    /// Starting block number
    pub block_number: u64,

    #[arg(short = 'n', long, require_equals = true, default_value_t = 1)]
    /// Number of blocks to build in a single proof
    pub block_count: u64,

    #[arg(short = 'c', long, require_equals = true, value_enum)]
    /// Which chain spec to use.
    pub chain: Option<NamedChain>,
}

#[derive(Debug, Clone, ValueEnum, Hash, Ord, PartialOrd, Eq, PartialEq)]
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

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    #[arg(flatten)]
    pub build_args: BuildArgs,

    #[arg(short = 'e', long, require_equals = true, default_value_t = 20)]
    /// The maximum cycle count of a segment as a power of 2
    pub execution_po2: u32,

    #[arg(short = 'p', long, default_value_t = false)]
    /// Save the profile of the execution in the current working directory
    pub profile: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ProveArgs {
    #[arg(flatten)]
    pub run_args: RunArgs,

    #[arg(short = 's', long, default_value_t = false)]
    /// Convert the resulting STARK receipt into a Groth-16 SNARK
    pub snark: bool,
}

#[derive(Args, Debug, Clone)]
pub struct VerifyArgs {
    #[arg(flatten)]
    pub build_args: BuildArgs,

    #[arg(short = 'f', long, require_equals = true)]
    /// Receipt file path
    pub file: PathBuf,
}
