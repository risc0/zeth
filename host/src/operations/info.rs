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

use alloy_sol_types::SolInterface;
use log::warn;
use zeth_lib::{
    consts::Network,
    host::{
        cache_file_path,
        provider::{new_provider, BlockQuery},
    },
    optimism::OpSystemInfo,
};

use crate::cli::Cli;

pub async fn op_info(cli: Cli) -> anyhow::Result<()> {
    let core_args = cli.core_args().clone();
    // Fetch all of the initial data
    let rpc_cache = core_args.cache.as_ref().map(|dir| {
        cache_file_path(
            dir,
            &core_args.network.to_string(),
            core_args.block_number,
            "json.gz",
        )
    });

    if core_args.network != Network::Optimism {
        warn!("Network automatically switched to optimism for this command.")
    }

    let op_block = tokio::task::spawn_blocking(move || {
        let mut provider = new_provider(rpc_cache, core_args.op_rpc_url.clone())
            .expect("Could not create provider");

        let op_block = provider
            .get_full_block(&BlockQuery {
                block_no: core_args.block_number,
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
