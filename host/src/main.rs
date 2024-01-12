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

extern crate core;

use std::{collections::VecDeque, fmt::Debug};

use alloy_sol_types::SolInterface;
use anyhow::{Context, Result};
use clap::Parser;
// use bonsai_sdk::alpha as bonsai_sdk;
use ethers_core::types::Transaction as EthersTransaction;
use log::{error, info, warn};
use risc0_zkvm::{
    default_prover, serde::to_vec, Assumption, ExecutorEnv, ExecutorImpl, FileSegmentRef, Receipt,
    Session,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tempfile::tempdir;
use zeth::{cli::Cli, *};
use zeth_guests::*;
use zeth_lib::{
    builder::{BlockBuilderStrategy, EthereumStrategy, OptimismStrategy},
    consts::{ChainSpec, Network, ETH_MAINNET_CHAIN_SPEC, OP_MAINNET_CHAIN_SPEC},
    host::{
        preflight::Preflight,
        provider::{new_provider, BlockQuery},
        rpc_db::RpcDb,
        verify::Verifier,
    },
    input::Input,
    optimism::{
        batcher_db::BatcherDb,
        composition::{ComposeInput, ComposeInputOperation, ComposeOutputOperation},
        config::OPTIMISM_CHAIN_SPEC,
        DeriveInput, DeriveMachine, OpSystemInfo,
    },
};
use zeth_primitives::{block::Header, tree::MerkleMountainRange};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    // Run simple debug info command
    if let Cli::OpInfo(..) = &cli {
        return op_info(cli).await;
    }

    // Execute other commands
    let core_args = cli.core_args();
    let sys_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let file_reference = format!("{}_{}", sys_time.as_secs(), cli.to_string());

    match core_args.network {
        Network::Ethereum => {
            let rpc_url = core_args.eth_rpc_url.clone();
            mono_chain::<EthereumStrategy>(
                cli,
                &file_reference,
                rpc_url,
                ETH_MAINNET_CHAIN_SPEC.clone(),
                ETH_BLOCK_ELF,
            )
            .await
        }
        Network::Optimism => {
            let rpc_url = core_args.op_rpc_url.clone();
            mono_chain::<OptimismStrategy>(
                cli,
                &file_reference,
                rpc_url,
                OP_MAINNET_CHAIN_SPEC.clone(),
                OP_BLOCK_ELF,
            )
            .await
        }
        Network::OptimismDerived => {
            if let Some(composition_size) = cli.composition() {
                multi_chain_compose(cli, composition_size, &file_reference).await
            } else {
                multi_chain_derive(cli, &file_reference).await
            }
        }
    }
}

async fn op_info(cli: Cli) -> Result<()> {
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

async fn mono_chain<N: BlockBuilderStrategy>(
    cli: Cli,
    file_reference: &String,
    rpc_url: Option<String>,
    chain_spec: ChainSpec,
    guest_elf: &[u8],
) -> Result<()>
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

    match &cli {
        Cli::Build(..) => {}
        Cli::Run(run_args) => {
            execute(
                &input,
                run_args.exec_args.local_exec,
                run_args.exec_args.profile,
                guest_elf,
                &preflight_data.header.hash(),
                file_reference,
            );
        }
        Cli::Prove(..) => {
            maybe_prove(
                &cli,
                &input,
                guest_elf,
                &preflight_data.header.hash(),
                vec![],
                file_reference,
                None,
            );
        }
        Cli::Verify(..) => {
            unimplemented!()
        }
        Cli::OpInfo(..) => {
            unreachable!()
        }
    }

    // let mut bonsai_session_uuid = args.verify_receipt_bonsai_uuid;

    // Run in Bonsai (if requested)
    // if bonsai_session_uuid.is_none() && args.submit_to_bonsai {
    //     info!("Creating Bonsai client");
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //
    //     // create the memoryImg, upload it and return the imageId
    //     info!("Uploading memory image");
    //     let img_id = {
    //         let program = Program::load_elf(guest_elf, risc0_zkvm::GUEST_MAX_MEM as u32)
    //             .expect("Could not load ELF");
    //         let image = MemoryImage::new(&program, risc0_zkvm::PAGE_SIZE as u32)
    //             .expect("Could not create memory image");
    //         let image_id = hex::encode(image.compute_id());
    //         let image = bincode::serialize(&image).expect("Failed to serialize memory
    // img");
    //
    //         client
    //             .upload_img(&image_id, image)
    //             .expect("Could not upload ELF");
    //         image_id
    //     };
    //
    //     // Prepare input data and upload it.
    //     info!("Uploading inputs");
    //     let input_data = to_vec(&input).unwrap();
    //     let input_data = bytemuck::cast_slice(&input_data).to_vec();
    //     let input_id = client
    //         .upload_input(input_data)
    //         .expect("Could not upload inputs");
    //
    //     // Start a session running the prover
    //     info!("Starting session");
    //     let session = client
    //         .create_session(img_id, input_id)
    //         .expect("Could not create Bonsai session");
    //
    //     println!("Bonsai session UUID: {}", session.uuid);
    //     bonsai_session_uuid = Some(session.uuid)
    // }

    // Verify receipt from Bonsai (if requested)
    // if let Some(session_uuid) = bonsai_session_uuid {
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //     let session = bonsai_sdk::SessionId { uuid: session_uuid };
    //
    //     loop {
    //         let res = session
    //             .status(&client)
    //             .expect("Could not fetch Bonsai status");
    //         if res.status == "RUNNING" {
    //             println!(
    //                 "Current status: {} - state: {} - continue polling...",
    //                 res.status,
    //                 res.state.unwrap_or_default()
    //             );
    //             tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    //             continue;
    //         }
    //         if res.status == "SUCCEEDED" {
    //             // Download the receipt, containing the output
    //             let receipt_url = res
    //                 .receipt_url
    //                 .expect("API error, missing receipt on completed session");
    //
    //             let receipt_buf = client
    //                 .download(&receipt_url)
    //                 .expect("Could not download receipt");
    //             let receipt: Receipt =
    //                 bincode::deserialize(&receipt_buf).expect("Could not deserialize
    // receipt");             receipt
    //                 .verify(guest_id)
    //                 .expect("Receipt verification failed");
    //
    //             let expected_hash = preflight_data.header.hash();
    //             let found_hash: BlockHash = receipt.journal.decode().unwrap();
    //
    //             if found_hash == expected_hash {
    //                 info!("Block hash (from Bonsai): {}", found_hash);
    //             } else {
    //                 error!(
    //                     "Final block hash mismatch (from Bonsai) {} (expected {})",
    //                     found_hash, expected_hash,
    //                 );
    //             }
    //         } else {
    //             panic!(
    //                 "Workflow exited: {} - | err: {}",
    //                 res.status,
    //                 res.error_msg.unwrap_or_default()
    //             );
    //         }
    //
    //         break;
    //     }
    // }

    Ok(())
}

async fn multi_chain_derive(cli: Cli, file_reference: &String) -> Result<()> {
    info!("Fetching data ...");
    let core_args = cli.core_args().clone();
    let (derive_input, output) = tokio::task::spawn_blocking(move || {
        let derive_input = DeriveInput {
            db: RpcDb::new(
                core_args.eth_rpc_url.clone(),
                core_args.op_rpc_url.clone(),
                core_args.cache.clone(),
            ),
            op_head_block_no: core_args.block_number,
            op_derive_block_count: core_args.block_count,
        };
        let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
            .context("Could not create derive machine")?;
        let derive_output = derive_machine.derive().context("could not derive")?;
        let derive_input_mem = DeriveInput {
            db: derive_machine.derive_input.db.get_mem_db(),
            op_head_block_no: core_args.block_number,
            op_derive_block_count: core_args.block_count,
        };
        let out: Result<_> = Ok((derive_input_mem, derive_output));
        out
    })
    .await?
    .context("preflight failed")?;

    info!("Running from memory ...");
    {
        let output_mem = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input.clone())
            .context("Could not create derive machine")?
            .derive()
            .unwrap();
        assert_eq!(output, output_mem);
    }

    info!("In-memory test complete");
    println!("Eth tail: {} {}", output.eth_tail.0, output.eth_tail.1);
    println!("Op Head: {} {}", output.op_head.0, output.op_head.1);
    for derived_block in &output.derived_op_blocks {
        println!("Derived: {} {}", derived_block.0, derived_block.1);
    }

    match &cli {
        Cli::Build(..) => {}
        Cli::Run(run_args) => {
            execute(
                &derive_input,
                run_args.exec_args.local_exec,
                run_args.exec_args.profile,
                OP_DERIVE_ELF,
                &output,
                file_reference,
            );
        }
        Cli::Prove(..) => {
            maybe_prove(
                &cli,
                &derive_input,
                OP_DERIVE_ELF,
                &output,
                vec![],
                file_reference,
                None,
            );
        }
        Cli::Verify(..) => {
            unimplemented!()
        }
        Cli::OpInfo(..) => {
            unreachable!()
        }
    }

    // let mut bonsai_session_uuid = args.verify_receipt_bonsai_uuid;

    // Run in Bonsai (if requested)
    // if bonsai_session_uuid.is_none() && args.submit_to_bonsai {
    //     info!("Creating Bonsai client");
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //
    //     // create the memoryImg, upload it and return the imageId
    //     info!("Uploading memory image");
    //     let img_id = {
    //         let program = Program::load_elf(OP_DERIVE_ELF, risc0_zkvm::GUEST_MAX_MEM as
    // u32)             .expect("Could not load ELF");
    //         let image = MemoryImage::new(&program, risc0_zkvm::PAGE_SIZE as u32)
    //             .expect("Could not create memory image");
    //         let image_id = hex::encode(image.compute_id());
    //         let image = bincode::serialize(&image).expect("Failed to serialize memory
    // img");
    //
    //         client
    //             .upload_img(&image_id, image)
    //             .expect("Could not upload ELF");
    //         image_id
    //     };
    //
    //     // Prepare input data and upload it.
    //     info!("Uploading inputs");
    //     let input_data = to_vec(&derive_input).unwrap();
    //     let input_data = bytemuck::cast_slice(&input_data).to_vec();
    //     let input_id = client
    //         .upload_input(input_data)
    //         .expect("Could not upload inputs");
    //
    //     // Start a session running the prover
    //     info!("Starting session");
    //     let session = client
    //         .create_session(img_id, input_id)
    //         .expect("Could not create Bonsai session");
    //
    //     println!("Bonsai session UUID: {}", session.uuid);
    //     bonsai_session_uuid = Some(session.uuid)
    // }

    // Verify receipt from Bonsai (if requested)
    // if let Some(session_uuid) = bonsai_session_uuid {
    //     let client = bonsai_sdk::Client::from_env(risc0_zkvm::VERSION)
    //         .expect("Could not create Bonsai client");
    //     let session = bonsai_sdk::SessionId { uuid: session_uuid };
    //
    //     loop {
    //         let res = session
    //             .status(&client)
    //             .expect("Could not fetch Bonsai status");
    //         if res.status == "RUNNING" {
    //             println!(
    //                 "Current status: {} - state: {} - continue polling...",
    //                 res.status,
    //                 res.state.unwrap_or_default()
    //             );
    //             tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    //             continue;
    //         }
    //         if res.status == "SUCCEEDED" {
    //             // Download the receipt, containing the output
    //             let receipt_url = res
    //                 .receipt_url
    //                 .expect("API error, missing receipt on completed session");
    //
    //             let receipt_buf = client
    //                 .download(&receipt_url)
    //                 .expect("Could not download receipt");
    //             let receipt: Receipt =
    //                 bincode::deserialize(&receipt_buf).expect("Could not deserialize
    // receipt");             receipt
    //                 .verify(OP_DERIVE_ID)
    //                 .expect("Receipt verification failed");
    //
    //             let bonsai_output: DeriveOutput = receipt.journal.decode().unwrap();
    //
    //             if output == bonsai_output {
    //                 println!("Bonsai succeeded");
    //             } else {
    //                 error!(
    //                     "Output mismatch! Bonsai: {:?}, expected: {:?}",
    //                     bonsai_output, output,
    //                 );
    //             }
    //         } else {
    //             panic!(
    //                 "Workflow exited: {} - | err: {}",
    //                 res.status,
    //                 res.error_msg.unwrap_or_default()
    //             );
    //         }
    //
    //         break;
    //     }
    //
    //     info!("Bonsai request completed");
    // }
    Ok(())
}

async fn multi_chain_compose(
    cli: Cli,
    composition_size: u64,
    file_reference: &String,
) -> Result<()> {
    let core_args = cli.core_args().clone();
    // OP Composition
    info!("Fetching data ...");
    let mut lift_queue = Vec::new();
    let mut receipt_index = 0;
    let mut eth_chain: Vec<Header> = Vec::new();
    for op_block_index in (0..core_args.block_count).step_by(composition_size as usize) {
        let db = RpcDb::new(
            core_args.eth_rpc_url.clone(),
            core_args.op_rpc_url.clone(),
            core_args.cache.clone(),
        );
        let (input, output, chain) = tokio::task::spawn_blocking(move || {
            let derive_input = DeriveInput {
                db,
                op_head_block_no: core_args.block_number + op_block_index,
                op_derive_block_count: composition_size,
            };
            let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
                .expect("Could not create derive machine");
            let eth_head_no = derive_machine.op_batcher.state.epoch.number;
            let eth_head = derive_machine
                .derive_input
                .db
                .get_eth_block_header(eth_head_no)
                .context("could not fetch eth head")?;
            let derive_output = derive_machine.derive().context("could not derive")?;
            let eth_tail = derive_machine
                .derive_input
                .db
                .get_eth_block_header(derive_output.eth_tail.0)
                .context("could not fetch eth tail")?;
            let mut eth_chain = vec![eth_head];
            for block_no in (eth_head_no + 1)..eth_tail.number {
                let eth_block = derive_machine
                    .derive_input
                    .db
                    .get_eth_block_header(block_no)
                    .context("could not fetch eth block")?;
                eth_chain.push(eth_block);
            }
            eth_chain.push(eth_tail);

            let derive_input_mem = DeriveInput {
                db: derive_machine.derive_input.db.get_mem_db(),
                op_head_block_no: core_args.block_number + op_block_index,
                op_derive_block_count: composition_size,
            };
            let out: anyhow::Result<_> = Ok((derive_input_mem, derive_output, eth_chain));
            out
        })
        .await??;

        info!("Deriving ...");
        {
            let output_mem = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, input.clone())
                .expect("Could not create derive machine")
                .derive()
                .unwrap();
            assert_eq!(output, output_mem);
        }

        let receipt = maybe_prove(
            &cli,
            &input,
            OP_DERIVE_ELF,
            &output,
            vec![],
            file_reference,
            Some(&mut receipt_index),
        );

        // Append derivation outputs to lift queue
        lift_queue.push((output, receipt));
        // Extend block chain
        for block in chain {
            let tail_num = match eth_chain.last() {
                None => 0u64,
                Some(tail) => tail.number,
            };
            // This check should be sufficient
            if tail_num < block.number {
                eth_chain.push(block);
            }
        }
    }

    // OP Composition
    // Prep
    let mut sibling_map = Default::default();
    let mut eth_mountain_range: MerkleMountainRange = Default::default();
    for block in &eth_chain {
        eth_mountain_range.logged_append_leaf(block.hash().0, &mut sibling_map);
    }
    let eth_chain_root = eth_mountain_range
        .logged_root(&mut sibling_map)
        .expect("No eth blocks loaded!");
    let prep_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::PREP {
            eth_blocks: eth_chain,
            mountain_range: Default::default(),
            prior: None,
        },
        eth_chain_root,
    };
    info!("Preparing ...");
    let prep_compose_output = prep_compose_input.clone().process();

    let prep_compose_receipt = maybe_prove(
        &cli,
        &prep_compose_input,
        OP_COMPOSE_ELF,
        &prep_compose_output,
        vec![],
        file_reference,
        Some(&mut receipt_index),
    );

    // Lift
    let mut join_queue = VecDeque::new();
    for (derive_output, derive_receipt) in lift_queue {
        let eth_tail_hash = derive_output.eth_tail.1 .0;
        let lift_compose_input = ComposeInput {
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::LIFT {
                derivation: derive_output,
                eth_tail_proof: MerkleMountainRange::proof(&sibling_map, eth_tail_hash),
            },
            eth_chain_root,
        };
        info!("Lifting ...");
        let lift_compose_output = lift_compose_input.clone().process();

        let lift_compose_receipt = if let Some(receipt) = derive_receipt {
            maybe_prove(
                &cli,
                &lift_compose_input,
                OP_COMPOSE_ELF,
                &lift_compose_output,
                vec![receipt.into()],
                file_reference,
                Some(&mut receipt_index),
            )
        } else {
            None
        };

        join_queue.push_back((lift_compose_output, lift_compose_receipt));
    }

    // Join
    while join_queue.len() > 1 {
        let (left, left_receipt) = join_queue.pop_front().unwrap();
        let (right, _right_receipt) = join_queue.front().unwrap();
        let ComposeOutputOperation::AGGREGATE {
            op_tail: left_op_tail,
            ..
        } = &left.operation
        else {
            panic!("Expected left aggregate operation output!")
        };
        let ComposeOutputOperation::AGGREGATE {
            op_head: right_op_head,
            ..
        } = &right.operation
        else {
            panic!("Expected right aggregate operation output!")
        };
        // Push dangling workloads (odd block count) to next round
        if left_op_tail != right_op_head {
            join_queue.push_back((left, left_receipt));
            continue;
        }
        // Pair up join
        let (right, right_receipt) = join_queue.pop_front().unwrap();
        let join_compose_input = ComposeInput {
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::JOIN { left, right },
            eth_chain_root,
        };
        info!("Joining ...");
        let join_compose_output = join_compose_input.clone().process();

        let join_compose_receipt =
            if let (Some(left_receipt), Some(right_receipt)) = (left_receipt, right_receipt) {
                maybe_prove(
                    &cli,
                    &join_compose_input,
                    OP_COMPOSE_ELF,
                    &join_compose_output,
                    vec![left_receipt.into(), right_receipt.into()],
                    file_reference,
                    Some(&mut receipt_index),
                )
            } else {
                None
            };

        // Send workload to next round
        join_queue.push_back((join_compose_output, join_compose_receipt));
    }

    // Finish
    let (aggregate_output, aggregate_receipt) = join_queue.pop_front().unwrap();
    let finish_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::FINISH {
            prep: prep_compose_output,
            aggregate: aggregate_output,
        },
        eth_chain_root,
    };
    info!("Finishing ...");
    let finish_compose_output = finish_compose_input.clone().process();

    let op_compose_receipt = if let (Some(prep_receipt), Some(aggregate_receipt)) =
        (prep_compose_receipt, aggregate_receipt)
    {
        maybe_prove(
            &cli,
            &finish_compose_input,
            OP_COMPOSE_ELF,
            &finish_compose_output,
            vec![prep_receipt.into(), aggregate_receipt.into()],
            file_reference,
            Some(&mut receipt_index),
        )
    } else {
        None
    };

    dbg!(&finish_compose_output);

    if let Some(final_receipt) = op_compose_receipt {
        final_receipt
            .verify(OP_COMPOSE_ID)
            .expect("Failed to verify final receipt");
        info!("Verified final receipt!");
    }

    Ok(())
}

fn maybe_prove<I: Serialize, O: Eq + Debug + DeserializeOwned>(
    cli: &Cli,
    input: &I,
    elf: &[u8],
    expected_output: &O,
    assumptions: Vec<Assumption>,
    file_reference: &String,
    receipt_index: Option<&mut usize>,
) -> Option<Receipt> {
    if let Cli::Prove(prove_args) = cli {
        if prove_args.submit_to_bonsai {
            unimplemented!()
        }
        // run prover
        let receipt = prove(
            prove_args.exec_args.local_exec,
            to_vec(input).expect("Could not serialize composition prep input!"),
            elf,
            assumptions,
            prove_args.exec_args.profile,
            file_reference,
        );
        // verify output
        let output_guest: O = receipt.journal.decode().unwrap();
        if expected_output == &output_guest {
            info!("Executor succeeded");
        } else {
            error!(
                "Output mismatch! Executor: {:?}, expected: {:?}",
                output_guest, expected_output,
            );
        }
        // save receipt
        save_receipt(file_reference, &receipt, receipt_index);
        // return result
        Some(receipt)
    } else {
        None
    }
}

fn prove(
    segment_limit_po2: u32,
    encoded_input: Vec<u32>,
    elf: &[u8],
    assumptions: Vec<Assumption>,
    profile: bool,
    file_reference: &String,
) -> Receipt {
    info!("Proving with segment_limit_po2 = {:?}", segment_limit_po2);
    info!(
        "Input size: {} words ( {} MB )",
        encoded_input.len(),
        encoded_input.len() * 4 / 1_000_000
    );

    info!("Running the prover...");
    let mut env_builder = ExecutorEnv::builder();

    env_builder
        .session_limit(None)
        .segment_limit_po2(segment_limit_po2)
        .write_slice(&encoded_input);

    if profile {
        info!("Profiling enabled.");
        env_builder.enable_profiler(format!("profile_{}.pb", file_reference));
    }

    for assumption in assumptions {
        env_builder.add_assumption(assumption);
    }

    let prover = default_prover();
    prover.prove(env_builder.build().unwrap(), elf).unwrap()
}

fn execute<T: serde::Serialize + ?Sized, O: Eq + Debug + DeserializeOwned>(
    input: &T,
    segment_limit_po2: u32,
    profile: bool,
    elf: &[u8],
    expected_output: &O,
    file_reference: &String,
) -> Session {
    info!(
        "Running in executor with segment_limit_po2 = {:?}",
        segment_limit_po2
    );

    let input = to_vec(input).expect("Could not serialize input!");
    info!(
        "Input size: {} words ( {} MB )",
        input.len(),
        input.len() * 4 / 1_000_000
    );

    info!("Running the executor...");
    let start_time = std::time::Instant::now();
    let session = {
        let mut builder = ExecutorEnv::builder();
        builder
            .session_limit(None)
            .segment_limit_po2(segment_limit_po2)
            .write_slice(&input);

        if profile {
            info!("Profiling enabled.");
            builder.enable_profiler(format!("profile_{}.pb", file_reference));
        }

        let env = builder.build().unwrap();
        let mut exec = ExecutorImpl::from_elf(env, elf).unwrap();

        let segment_dir = tempdir().unwrap();

        exec.run_with_callback(|segment| {
            Ok(Box::new(FileSegmentRef::new(&segment, segment_dir.path())?))
        })
        .unwrap()
    };
    // verify output
    let output_guest: O = session.journal.clone().unwrap().decode().unwrap();
    if expected_output == &output_guest {
        info!("Executor succeeded");
    } else {
        error!(
            "Output mismatch! Executor: {:?}, expected: {:?}",
            output_guest, expected_output,
        );
    }
    // report performance
    println!(
        "Generated {:?} segments; elapsed time: {:?}",
        session.segments.len(),
        start_time.elapsed()
    );
    println!(
        "Executor ran in (roughly) {} cycles",
        session.segments.len() * (1 << segment_limit_po2)
    );
    // return result
    session
}
