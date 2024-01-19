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

use std::collections::VecDeque;

use anyhow::Context;
use log::info;
use zeth_guests::*;
use zeth_lib::{
    builder::OptimismStrategy,
    consts::{Network, OP_MAINNET_CHAIN_SPEC},
    host::{preflight::Preflight, rpc_db::RpcDb},
    input::Input,
    optimism::{
        batcher_db::BatcherDb,
        composition::{ComposeInput, ComposeInputOperation, ComposeOutputOperation},
        config::OPTIMISM_CHAIN_SPEC,
        DeriveInput, DeriveMachine,
    },
};
use zeth_primitives::{
    block::Header,
    transactions::optimism::OptimismTxEssence,
    tree::{MerkleMountainRange, MerkleProof},
};

use crate::{
    cache_file_path,
    cli::{Cli, CoreArgs},
    operations::{execute, maybe_prove},
};

async fn fetch_op_blocks(
    core_args: &CoreArgs,
    block_number: u64,
    block_count: u64,
) -> anyhow::Result<Vec<Input<OptimismTxEssence>>> {
    let mut op_blocks = vec![];
    for i in 0..block_count {
        let block_number = block_number + i;
        let rpc_cache = core_args.cache.as_ref().map(|dir| {
            cache_file_path(dir, &Network::Optimism.to_string(), block_number, "json.gz")
        });
        let rpc_url = core_args.op_rpc_url.clone();
        // Collect block building data
        let preflight_result = tokio::task::spawn_blocking(move || {
            OptimismStrategy::run_preflight(
                OP_MAINNET_CHAIN_SPEC.clone(),
                rpc_cache,
                rpc_url,
                block_number,
            )
        })
        .await?
        .context("preflight failed")?;

        // Create the guest input from [Init]
        let input = preflight_result
            .clone()
            .try_into()
            .context("invalid preflight data")?;

        op_blocks.push(input);
    }

    Ok(op_blocks)
}

pub async fn derive_rollup_blocks(cli: Cli, file_reference: &String) -> anyhow::Result<()> {
    info!("Fetching data ...");
    let core_args = cli.core_args().clone();
    let op_blocks = fetch_op_blocks(
        &core_args,
        core_args.block_number + 1,
        core_args.block_count,
    )
    .await?;

    let (derive_input, output) = tokio::task::spawn_blocking(move || {
        let derive_input = DeriveInput {
            db: RpcDb::new(
                core_args.eth_rpc_url.clone(),
                core_args.op_rpc_url.clone(),
                core_args.cache.clone(),
            ),
            op_head_block_no: core_args.block_number,
            op_derive_block_count: core_args.block_count,
            op_blocks: op_blocks.clone(),
        };
        let mut derive_machine = DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input)
            .context("Could not create derive machine")?;
        let derive_output = derive_machine.derive().context("could not derive")?;
        let derive_input_mem = DeriveInput {
            db: derive_machine.derive_input.db.get_mem_db(),
            op_head_block_no: core_args.block_number,
            op_derive_block_count: core_args.block_count,
            op_blocks,
        };
        let out: anyhow::Result<_> = Ok((derive_input_mem, derive_output));
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

pub async fn compose_derived_rollup_blocks(
    cli: Cli,
    composition_size: u64,
    file_reference: &String,
) -> anyhow::Result<()> {
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
        let op_head_block_no = core_args.block_number + op_block_index;
        let op_blocks = fetch_op_blocks(&core_args, op_head_block_no + 1, composition_size).await?;

        let (input, output, chain) = tokio::task::spawn_blocking(move || {
            let derive_input = DeriveInput {
                db,
                op_head_block_no: core_args.block_number + op_block_index,
                op_derive_block_count: composition_size,
                op_blocks: op_blocks.clone(),
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
                op_blocks,
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
        eth_mountain_range.append_leaf(block.hash().0, Some(&mut sibling_map));
    }
    let eth_chain_root = eth_mountain_range
        .root(Some(&mut sibling_map))
        .expect("No eth blocks loaded!");
    let prep_compose_input = ComposeInput {
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::PREP {
            eth_blocks: eth_chain,
            prior_prep: None,
        },
        eth_chain_merkle_root: eth_chain_root,
    };
    info!("Preparing ...");
    let prep_compose_output = prep_compose_input
        .clone()
        .process()
        .expect("Prep composition failed.");

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
                eth_tail_proof: MerkleProof::new(&sibling_map, eth_tail_hash),
            },
            eth_chain_merkle_root: eth_chain_root,
        };
        info!("Lifting ...");
        let lift_compose_output = lift_compose_input
            .clone()
            .process()
            .expect("Lift composition failed.");

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
            eth_chain_merkle_root: eth_chain_root,
        };
        info!("Joining ...");
        let join_compose_output = join_compose_input
            .clone()
            .process()
            .expect("Join composition failed.");

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
        eth_chain_merkle_root: eth_chain_root,
    };
    info!("Finishing ...");
    let finish_compose_output = finish_compose_input
        .clone()
        .process()
        .expect("Finish composition failed.");

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
