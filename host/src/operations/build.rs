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
use log::{info, warn};
use risc0_zkvm::{compute_image_id, Receipt, Session};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use zeth_lib::{
    builder::BlockBuilderStrategy,
    consts::ChainSpec,
    host::{cache_file_path, preflight::Preflight, verify::Verifier},
    input::BlockBuildInput,
    output::BlockBuildOutput,
};

const MAX_CONCURRENT_REQUESTS: usize = 5;

use crate::{
    cli::{BuildArgs, Cli, RunArgs},
    operations::{execute, maybe_prove, verify_bonsai_receipt},
};

/// Build a single block using the specified strategy.
async fn preflight_block<N: BlockBuilderStrategy>(
    build_args: BuildArgs,
    current_block: u64,
    rpc_url: Option<String>,
    chain_spec: Arc<ChainSpec>,
) -> anyhow::Result<(BlockBuildInput<N::TxEssence>, BlockBuildOutput)>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    // Fetch all of the initial data
    let rpc_cache = build_args.cache.as_ref().map(|dir| {
        cache_file_path(
            dir,
            &build_args.network.to_string(),
            build_args.block_number,
            "json.gz",
        )
    });

    let init_spec = chain_spec.clone();
    let preflight_result = tokio::task::spawn_blocking(move || {
        N::preflight_with_external_data(&init_spec, rpc_cache, rpc_url, current_block)
    })
    .await?;
    let preflight_data = preflight_result.context("preflight failed")?;

    // Create the guest input from [Init]
    let input: BlockBuildInput<N::TxEssence> = preflight_data
        .clone()
        .try_into()
        .context("invalid preflight data")?;

    // Verify that the transactions run correctly
    info!("Running from memory ...");
    let output = N::build_from(&chain_spec, input.clone()).context("Error while building block")?;

    match &output {
        BlockBuildOutput::SUCCESS {
            hash, head, state, ..
        } => {
            info!("Verifying final state using provider data ...");
            preflight_data.verify_block(head, state)?;

            info!("Final block hash derived successfully. {}", hash);
        }
        BlockBuildOutput::FAILURE { .. } => {
            warn!("Proving bad block construction!")
        }
    }

    Ok((input, output))
}

// TODO use this
enum ExecuteBlockResult {
    Run(Session),
    Prove((String, Receipt)),
}

/// Build a single block using the specified strategy.
async fn execute_block<N: BlockBuilderStrategy>(
    input: BlockBuildInput<N::TxEssence>,
    output: BlockBuildOutput,
    cli: Arc<Cli>,
    guest_elf: &'static [u8],
    session_channel: Option<mpsc::UnboundedSender<(u64, u64, u64)>>,
    // TODO hack for expedience, can pass this cleaner by updating the return type to an enum (and remove above)
    block_num: u64,
) -> anyhow::Result<Option<(String, Receipt)>>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    let compressed_output = output.with_state_hashed();
    let result = match &*cli {
        Cli::Build(..) => None,
        Cli::Run(run_args) => {
            let session = execute(
                &input,
                run_args.execution_po2,
                run_args.profile,
                &guest_elf,
                &compressed_output,
                &cli.execution_tag(),
            );

            if let Some(sender) = session_channel {
                sender.send((block_num, session.user_cycles, session.total_cycles))?;
            }
            None
        }
        Cli::Prove(..) => {
            maybe_prove(
                &cli,
                &input,
                &guest_elf,
                &compressed_output,
                Default::default(),
            )
            .await
        }
        Cli::Verify(verify_args) => Some(
            verify_bonsai_receipt(
                compute_image_id(&guest_elf)?,
                &compressed_output,
                verify_args.bonsai_receipt_uuid.clone(),
                4,
            )
            .await?,
        ),
    };
    Ok(result)
}

/// Build a single block using the specified strategy.
pub async fn build_block<N: BlockBuilderStrategy>(
    cli: Arc<Cli>,
    rpc_url: Option<String>,
    chain_spec: Arc<ChainSpec>,
    guest_elf: &'static [u8],
) -> anyhow::Result<Vec<Option<(String, Receipt)>>>
where
    N::TxEssence:
        'static + Send + Sync + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    let build_args = cli.build_args().clone();

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    let mut join_handles = JoinSet::new();

    let (session_sender, writer_handle) = if let Cli::Run(RunArgs {
        export_csv: Some(csv_path),
        ..
    }) = &*cli
    {
        // TODO Session type isn't sync friendly, u64s are block num, user cycles, total cycles
        let (tx, mut rx) = mpsc::unbounded_channel::<(u64, u64, u64)>();
        let csv_path = csv_path.clone();
        let join_handle = tokio::spawn(async move {
            let mut file = File::create(csv_path).await.unwrap();
            while let Some((block_num, user_cycles, total_cycles)) = rx.recv().await {
                // Write csv record.
                file.write_all(
                    // TODO remove leading ' -- just used for google sheets
                    format!("'{},{},{}\n", block_num, user_cycles, total_cycles).as_bytes(),
                )
                .await
                .unwrap();
            }
        });
        (Some(tx), Some(join_handle))
    } else {
        (None, None)
    };

    let block_num = build_args.block_number;
    // TODO semantics are a bit mixed with block count (was OP specific)
    for num in block_num..block_num + build_args.block_count as u64 {
        // Acquire permit before queueing job.
        let semaphore = semaphore.clone();

        // Clone variables needed.
        let rpc_url = rpc_url.clone();
        let cli = cli.clone();
        let chain_spec = chain_spec.clone();
        let session_sender = session_sender.clone();

        // Spawn blocking for
        join_handles.spawn(async move {
            // Acquire permit before sending request.
            let _permit = semaphore.acquire().await.unwrap();

            let (input, output) =
                preflight_block::<N>(cli.build_args().clone(), num, rpc_url, chain_spec).await?;

            drop(_permit);

            // TODO this could be separated into a separate task, to make sure Bonsai also
            //  doesn't get throttled, for now just going quick path of dropping permit after
            //  preflight.
            let result =
                execute_block::<N>(input, output, cli, guest_elf, session_sender, num).await;

            result
        });
    }

    drop(session_sender);

    // Collect responses from tasks.
    let mut responses = Vec::new();
    // TODO hacky, should be one path if possible
    if let Some(mut writer_handle) = writer_handle {
        loop {
            tokio::select! {
                // Cancellation safety: `join_next` is cancel safe
                Some(val) = join_handles.join_next() => {
                    responses.push(val??);
                    continue;
                }
                // Cancellation safety: &mut JoinHandle is cancel safe
                val = &mut writer_handle  => {
                    val?;
                    break;
                }
                else  => { break }
            }
        }
    } else {
        while let Some(val) = join_handles.join_next().await {
            responses.push(val??);
        }
    }

    Ok(responses)
}
