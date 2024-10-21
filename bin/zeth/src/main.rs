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

use clap::Parser;
use reth_chainspec::MAINNET;
use zeth::cli::{Cli, Network};
use zeth::client::{RethZethClient, ZethClient};
use zeth_guests::{RETH_ELF, RETH_ID};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    // execute the command
    let build_args = cli.build_args();
    let (_image_id, _stark) = match build_args.network {
        Network::Ethereum => {
            let rpc_url = build_args.eth_rpc_url.clone();
            (
                RETH_ID,
                RethZethClient::build_block(&cli, rpc_url, MAINNET.clone(), RETH_ELF).await?,
            )
        }
        Network::Optimism => todo!(),
    };
    //
    // // Create/verify Groth16 SNARK
    // if cli.snark() {
    //     let Some((stark_uuid, stark_receipt)) = stark else {
    //         panic!("No STARK data to snarkify!");
    //     };
    //
    //     if !cli.submit_to_bonsai() {
    //         panic!("Bonsai submission flag required to create a SNARK!");
    //     }
    //
    //     let image_id = Digest::from(image_id);
    //     let (snark_uuid, snark_receipt) = stark2snark(image_id, stark_uuid, stark_receipt).await?;
    //
    //     info!("Validating SNARK uuid: {}", snark_uuid);
    //
    //     verify_groth16_snark(&cli, image_id, snark_receipt).await?;
    // }

    Ok(())
}
