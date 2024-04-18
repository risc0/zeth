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

use core::fmt;
use std::path::PathBuf;

use clap::ValueEnum;

#[derive(clap::Parser, Debug, Clone)]
#[command(name = "zeth")]
#[command(bin_name = "zeth")]
#[command(author, version, about, long_about = None)]
pub enum Cli {
    /// Build blocks only on the host
    Build(BuildArgs),
    /// Run the block building inside the executor
    Run(RunArgs),
    /// Provably build blocks inside the zkVM
    Prove(ProveArgs),
    /// Verify a block building receipt
    Verify(VerifyArgs),
}

impl Cli {
    pub fn build_args(&self) -> &BuildArgs {
        match &self {
            Cli::Build(build_args) => build_args,
            Cli::Run(run_args) => &run_args.build_args,
            Cli::Prove(prove_args) => &prove_args.run_args.build_args,
            Cli::Verify(..) => unimplemented!(),
        }
    }

    /// Generate a unique tag for the command execution
    pub fn execution_tag(&self) -> String {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        match &self {
            Cli::Build(args) => format!("{}_build_{}", time.as_secs(), args.tag()),
            Cli::Run(args) => format!("{}_run_{}", time.as_secs(), args.tag()),
            Cli::Prove(args) => format!("{}_prove_{}", time.as_secs(), args.tag()),
            Cli::Verify(..) => unimplemented!(),
        }
    }

    pub fn submit_to_bonsai(&self) -> bool {
        if let Cli::Prove(prove_args) = self {
            prove_args.submit_to_bonsai
        } else {
            false
        }
    }

    pub fn snark(&self) -> bool {
        if let Cli::Prove(prove_args) = self {
            prove_args.snark_args.snark
        } else {
            false
        }
    }

    pub fn verifier_or_eth_rpc_url(&self) -> Option<String> {
        let verifier_rpc_url = if let Cli::Prove(prove_args) = self {
            prove_args.snark_args.verifier_rpc_url.clone()
        } else {
            None
        };
        verifier_rpc_url.or(self.build_args().eth_rpc_url.clone())
    }

    pub fn verifier_contract(&self) -> Option<String> {
        if let Cli::Prove(prove_args) = self {
            prove_args.snark_args.verifier_contract.clone()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Network {
    /// Ethereum Mainnet
    Ethereum,
    /// Optimism Mainnet
    Optimism,
    /// Optimism Mainnet as derived from the Ethereum Mainnet
    OptimismDerived,
    /// Ganaghe Ethereum (Merge fork)
    GanacheMerge,
    /// Ganaghe Ethereum (Shanghai fork)
    GanacheShanghai,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // use the name of the clap::ValueEnum
        let val = self.to_possible_value().unwrap();
        write!(f, "{}", val.get_name())
    }
}

trait Tag {
    fn tag(&self) -> String;
}

#[derive(clap::Args, Debug, Clone)]
pub struct BuildArgs {
    #[clap(
        short = 'w',
        long,
        require_equals = true,
        value_enum,
        default_value_t = Network::Ethereum
    )]
    /// Network name
    pub network: Network,

    #[clap(short, long, require_equals = true)]
    /// URL of the Ethereum RPC node
    pub eth_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true)]
    /// URL of the Optimism RPC node
    pub op_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true, num_args = 0..=1, default_missing_value = "cache_rpc")]
    /// Cache RPC calls locally; the value specifies the cache directory
    ///
    /// [default when the flag is present: cache_rpc]
    pub cache: Option<PathBuf>,

    #[clap(short, long, require_equals = true)]
    /// Start block number
    pub block_number: u64,

    #[clap(short = 'n', long, require_equals = true, default_value_t = 1)]
    /// Number of blocks to derive (optimism-derived network only)
    pub block_count: u32,

    #[clap(short='m', long, require_equals = true, num_args = 0..=1, default_missing_value = "1")]
    /// Derive the Optimism blocks using proof composition (optimism-derived network
    /// only); the value specifies the the number of blocks to process per derivation call
    ///
    /// [default when the flag is present: 1]
    pub composition: Option<u32>,
}

impl Tag for BuildArgs {
    fn tag(&self) -> String {
        format!(
            "{}_{}_{}_{}",
            self.network,
            self.block_number,
            self.block_count,
            self.composition.unwrap_or_default()
        )
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    #[clap(flatten)]
    pub build_args: BuildArgs,

    #[clap(short = 'x', long, require_equals = true, default_value_t = 20)]
    /// The maximum cycle count of a segment as a power of 2
    pub execution_po2: u32,

    #[clap(short, long, default_value_t = false)]
    /// Whether to profile the zkVM execution
    pub profile: bool,
}

impl Tag for RunArgs {
    fn tag(&self) -> String {
        self.build_args.tag()
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct ProveArgs {
    #[clap(flatten)]
    pub run_args: RunArgs,

    #[clap(short, long, default_value_t = false)]
    /// Prove remotely using Bonsai
    pub submit_to_bonsai: bool,

    #[clap(flatten)]
    pub snark_args: SnarkArgs,
}

#[derive(clap::Args, Debug, Clone)]
pub struct SnarkArgs {
    /// Convert the resulting STARK receipt into a Groth-16 SNARK using Bonsai
    #[clap(short, long, default_value_t = false)]
    pub snark: bool,

    #[clap(short, long, require_equals = true)]
    /// URL of the Ethereum RPC node for SNARK verification.
    pub verifier_rpc_url: Option<String>,

    #[clap(short, long, require_equals = true)]
    /// Address of the RiscZeroGroth16Verifier contract. Requires `eth_rpc_url` or
    /// `verifier_rpc_url` to be set.
    pub verifier_contract: Option<String>,
}

impl Tag for ProveArgs {
    fn tag(&self) -> String {
        self.run_args.tag()
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct VerifyArgs {
    #[clap(short, long, require_equals = true)]
    /// Verify the receipt from the provided Bonsai Session UUID
    pub bonsai_receipt_uuid: String,
}
