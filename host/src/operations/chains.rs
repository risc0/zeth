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

use std::fmt::Debug;

use anyhow::Context;
use ethers_core::types::Transaction as EthersTransaction;
use log::info;
use risc0_zkvm::compute_image_id;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    builder::BlockBuilderStrategy,
    consts::ChainSpec,
    host::{preflight::Preflight, verify::Verifier},
    input::Input,
};

use crate::{
    cache_file_path,
    cli::Cli,
    operations::{execute, maybe_prove, verify_bonsai_receipt},
};

pub async fn build_chain_blocks<N: BlockBuilderStrategy>(
    cli: Cli,
    file_reference: &String,
    rpc_url: Option<String>,
    chain_spec: ChainSpec,
    guest_elf: &[u8],
) -> anyhow::Result<()>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
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

    let init_spec = chain_spec.clone();
    let preflight_result = tokio::task::spawn_blocking(move || {
        N::run_preflight(init_spec, rpc_cache, rpc_url, core_args.block_number)
    })
    .await?;
    let preflight_data = preflight_result.context("preflight failed")?;

    // Create the guest input from [Init]
    let input: Input<N::TxEssence> = preflight_data
        .clone()
        .try_into()
        .context("invalid preflight data")?;

    // Verify that the transactions run correctly
    info!("Running from memory ...");
    let (header, state_trie) =
        N::build_from(&chain_spec, input.clone()).context("Error while building block")?;

    info!("Verifying final state using provider data ...");
    preflight_data.verify_block(&header, &state_trie)?;

    info!("Final block hash derived successfully. {}", header.hash());

    let expected_output = preflight_data.header.hash();
    match &cli {
        Cli::Build(..) => {}
        Cli::Run(run_args) => {
            execute(
                &input,
                run_args.exec_args.local_exec,
                run_args.exec_args.profile,
                guest_elf,
                &expected_output,
                file_reference,
            );
        }
        Cli::Prove(..) => {
            maybe_prove(
                &cli,
                &input,
                guest_elf,
                &expected_output,
                Default::default(),
                file_reference,
                None,
            );
        }
        Cli::Verify(verify_args) => {
            verify_bonsai_receipt(
                compute_image_id(guest_elf)?,
                &expected_output,
                verify_args.bonsai_receipt_uuid.clone(),
                None,
            )?;
        }
        Cli::OpInfo(..) => {
            unreachable!()
        }
    }

    Ok(())
}
