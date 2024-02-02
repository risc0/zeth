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

use std::{
    borrow::BorrowMut,
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use log::info;
use risc0_zkvm::Assumption;
use zeth_guests::*;
use zeth_lib::{
    builder::{BlockBuilderStrategy, OptimismStrategy},
    consts::{Network, OP_MAINNET_CHAIN_SPEC},
    host::{rpc_db::RpcDb, ProviderFactory},
    input::BlockBuildInput,
    optimism::{
        batcher_db::BatcherDb,
        composition::{ComposeInput, ComposeInputOperation, ComposeOutputOperation},
        config::OPTIMISM_CHAIN_SPEC,
        DeriveInput, DeriveMachine,
    },
    output::BlockBuildOutput,
};
use zeth_primitives::{
    block::Header,
    mmr::{MerkleMountainRange, MerkleProof},
    transactions::optimism::OptimismTxEssence,
};

use crate::{cli::Cli, operations::maybe_prove};

pub async fn derive_rollup_blocks(cli: Cli, file_reference: &String) -> anyhow::Result<()> {
    info!("Fetching data ...");
    let core_args = cli.core_args().clone();
    let op_builder_provider_factory = ProviderFactory::new(
        core_args.cache.clone(),
        Network::Optimism,
        core_args.op_rpc_url.clone(),
    );
    let receipt_index = Arc::new(Mutex::new(0usize));

    info!("Running preflight");
    let derive_input = DeriveInput {
        db: RpcDb::new(
            core_args.eth_rpc_url.clone(),
            core_args.op_rpc_url.clone(),
            core_args.cache.clone(),
        ),
        op_head_block_no: core_args.block_number,
        op_derive_block_count: core_args.block_count,
        op_block_outputs: vec![],
        block_image_id: OP_BLOCK_ID,
    };
    let factory_clone = op_builder_provider_factory.clone();
    let (op_block_inputs, derive_machine, derive_output) = tokio::task::spawn_blocking(move || {
        let mut derive_machine =
            DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input, Some(factory_clone))
                .expect("Could not create derive machine");
        let mut op_block_inputs = vec![];

        let derive_output = derive_machine
            .derive(Some(&mut op_block_inputs))
            .expect("could not derive");
        (op_block_inputs, derive_machine, derive_output)
    })
    .await?;

    let (assumptions, op_block_outputs) =
        build_op_blocks(&cli, file_reference, receipt_index.clone(), op_block_inputs);

    let derive_input_mem = DeriveInput {
        db: derive_machine.derive_input.db.get_mem_db(),
        op_head_block_no: core_args.block_number,
        op_derive_block_count: core_args.block_count,
        op_block_outputs,
        block_image_id: OP_BLOCK_ID,
    };

    info!("Running from memory ...");
    {
        let input_clone = derive_input_mem.clone();
        let output_mem = tokio::task::spawn_blocking(move || {
            DeriveMachine::new(
                &OPTIMISM_CHAIN_SPEC,
                input_clone,
                Some(op_builder_provider_factory),
            )
            .expect("Could not create derive machine")
            .derive(None)
            .unwrap()
        })
        .await?;
        assert_eq!(derive_output, output_mem);
    }

    info!("In-memory test complete");
    println!(
        "Eth tail: {} {}",
        derive_output.eth_tail.number, derive_output.eth_tail.hash
    );
    println!(
        "Op Head: {} {}",
        derive_output.op_head.number, derive_output.op_head.hash
    );
    for derived_block in &derive_output.derived_op_blocks {
        println!("Derived: {} {}", derived_block.number, derived_block.hash);
    }

    maybe_prove(
        &cli,
        &derive_input_mem,
        OP_DERIVE_ELF,
        &derive_output,
        assumptions,
        file_reference,
        Some(receipt_index.lock().unwrap().borrow_mut()),
    );

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
    let receipt_index = Arc::new(Mutex::new(0usize));
    let mut complete_eth_chain: Vec<Header> = Vec::new();
    for op_block_index in (0..core_args.block_count).step_by(composition_size as usize) {
        let db = RpcDb::new(
            core_args.eth_rpc_url.clone(),
            core_args.op_rpc_url.clone(),
            core_args.cache.clone(),
        );
        let op_builder_provider_factory = ProviderFactory::new(
            core_args.cache.clone(),
            Network::Optimism,
            core_args.op_rpc_url.clone(),
        );

        let derive_input = DeriveInput {
            db,
            op_head_block_no: core_args.block_number + op_block_index,
            op_derive_block_count: composition_size,
            op_block_outputs: vec![],
            block_image_id: OP_BLOCK_ID,
        };
        let factory_clone = op_builder_provider_factory.clone();
        let mut derive_machine = tokio::task::spawn_blocking(move || {
            DeriveMachine::new(&OPTIMISM_CHAIN_SPEC, derive_input, Some(factory_clone))
                .expect("Could not create derive machine")
        })
        .await?;
        let eth_head_no = derive_machine.op_batcher.state.epoch.number;
        let eth_head = derive_machine
            .derive_input
            .db
            .get_full_eth_block(eth_head_no)
            .context("could not fetch eth head")?
            .block_header
            .clone();
        let (op_block_inputs, mut derive_machine, derive_output) =
            tokio::task::spawn_blocking(move || {
                let mut op_block_inputs = vec![];
                let derive_output = derive_machine
                    .derive(Some(&mut op_block_inputs))
                    .expect("could not derive");
                (op_block_inputs, derive_machine, derive_output)
            })
            .await?;

        let eth_tail = derive_machine
            .derive_input
            .db
            .get_full_eth_block(derive_output.eth_tail.number)
            .context("could not fetch eth tail")?
            .block_header
            .clone();
        let mut eth_chain = vec![eth_head];
        for block_no in (eth_head_no + 1)..eth_tail.number {
            eth_chain.push(
                derive_machine
                    .derive_input
                    .db
                    .get_full_eth_block(block_no)
                    .context("could not fetch eth block")?
                    .block_header
                    .clone(),
            );
        }
        eth_chain.push(eth_tail);

        let (assumptions, op_block_outputs) =
            build_op_blocks(&cli, file_reference, receipt_index.clone(), op_block_inputs);

        let derive_input_mem = DeriveInput {
            db: derive_machine.derive_input.db.get_mem_db(),
            op_head_block_no: core_args.block_number + op_block_index,
            op_derive_block_count: composition_size,
            op_block_outputs,
            block_image_id: OP_BLOCK_ID,
        };

        info!("Deriving ...");
        {
            let input_clone = derive_input_mem.clone();
            let output_mem = tokio::task::spawn_blocking(move || {
                DeriveMachine::new(
                    &OPTIMISM_CHAIN_SPEC,
                    input_clone,
                    Some(op_builder_provider_factory),
                )
                .expect("Could not create derive machine")
                .derive(None)
                .context("could not derive")
            })
            .await??;
            assert_eq!(derive_output, output_mem);
        }

        let receipt = maybe_prove(
            &cli,
            &derive_input_mem,
            OP_DERIVE_ELF,
            &derive_output,
            assumptions,
            file_reference,
            Some(receipt_index.lock().unwrap().borrow_mut()),
        );

        // Append derivation outputs to lift queue
        lift_queue.push((derive_output, receipt));
        // Extend block chain
        for block in eth_chain {
            let tail_num = match complete_eth_chain.last() {
                None => 0u64,
                Some(tail) => tail.number,
            };
            // This check should be sufficient
            if tail_num < block.number {
                complete_eth_chain.push(block);
            }
        }
    }

    // OP Composition
    // Prep
    let mut sibling_map = Default::default();
    let mut eth_mountain_range: MerkleMountainRange = Default::default();
    for block in &complete_eth_chain {
        eth_mountain_range.append_leaf(block.hash().0, Some(&mut sibling_map));
    }
    let eth_chain_root = eth_mountain_range
        .root(Some(&mut sibling_map))
        .expect("No eth blocks loaded!");
    let prep_compose_input = ComposeInput {
        block_image_id: OP_BLOCK_ID,
        derive_image_id: OP_DERIVE_ID,
        compose_image_id: OP_COMPOSE_ID,
        operation: ComposeInputOperation::PREP {
            eth_blocks: complete_eth_chain,
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
        Some(receipt_index.lock().unwrap().borrow_mut()),
    );

    // Lift
    let mut join_queue = VecDeque::new();
    for (derive_output, derive_receipt) in lift_queue {
        let eth_tail_hash = derive_output.eth_tail.hash.0;
        info!("Lifting ... {:?}", &derive_output);
        let lift_compose_input = ComposeInput {
            block_image_id: OP_BLOCK_ID,
            derive_image_id: OP_DERIVE_ID,
            compose_image_id: OP_COMPOSE_ID,
            operation: ComposeInputOperation::LIFT {
                derivation: derive_output,
                eth_tail_proof: MerkleProof::new(&sibling_map, eth_tail_hash),
            },
            eth_chain_merkle_root: eth_chain_root,
        };
        let lift_compose_output = lift_compose_input
            .clone()
            .process()
            .expect("Lift composition failed.");
        info!("Lifted ... {:?}", &lift_compose_output);

        let lift_compose_receipt = if let Some(receipt) = derive_receipt {
            maybe_prove(
                &cli,
                &lift_compose_input,
                OP_COMPOSE_ELF,
                &lift_compose_output,
                vec![receipt.into()],
                file_reference,
                Some(receipt_index.lock().unwrap().borrow_mut()),
            )
        } else {
            None
        };

        join_queue.push_back((lift_compose_output, lift_compose_receipt));
    }

    // Join
    while join_queue.len() > 1 {
        // Pop left output
        let (left, left_receipt) = join_queue.pop_front().unwrap();
        // Only peek at right output
        let (right, _right_receipt) = join_queue.front().unwrap();
        info!("Joining");
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
            info!(
                "Skipping dangling workload: {} - {}",
                left_op_tail.number, right_op_head.number
            );
            join_queue.push_back((left, left_receipt));
            continue;
        }
        // Actually pop right output for pairing
        let (right, right_receipt) = join_queue.pop_front().unwrap();
        let join_compose_input = ComposeInput {
            block_image_id: OP_BLOCK_ID,
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
                    Some(receipt_index.lock().unwrap().borrow_mut()),
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
        block_image_id: OP_BLOCK_ID,
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
            Some(receipt_index.lock().unwrap().borrow_mut()),
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

fn build_op_blocks(
    cli: &Cli,
    file_reference: &String,
    receipt_index: Arc<Mutex<usize>>,
    op_block_inputs: Vec<BlockBuildInput<OptimismTxEssence>>,
) -> (Vec<Assumption>, Vec<BlockBuildOutput>) {
    let mut assumptions: Vec<Assumption> = vec![];
    let mut op_block_outputs = vec![];
    for input in op_block_inputs {
        let output = OptimismStrategy::build_from(&OP_MAINNET_CHAIN_SPEC, input.clone())
            .expect("Failed to build op block")
            .with_state_hashed();

        if let Some(receipt) = maybe_prove(
            cli,
            &input,
            OP_BLOCK_ELF,
            &output,
            vec![],
            file_reference,
            Some(receipt_index.lock().unwrap().borrow_mut()),
        ) {
            assumptions.push(receipt.into());
        }
        op_block_outputs.push(output);
    }
    (assumptions, op_block_outputs)
}
