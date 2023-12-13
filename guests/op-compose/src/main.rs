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

#![no_main]

use risc0_zkvm::guest::env;
use risc0_zkvm::serde::to_vec;
use risc0_zkvm::sha::Digest;
use zeth_lib::optimism::{ComposeInput, ComposeInputOperation, ComposeOutput, MemDb};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    // read composition input
    let compose_input: ComposeInput<MemDb> = env::read();
    // process input
    let compose_output = match compose_input.operation {
        ComposeInputOperation::LIFT(derive_output) => {
            // Verify derivation receipt
            let derive_journal =
                to_vec(&derive_output).expect("Failed to encode expected derivation journal");
            env::verify(
                Digest::from(compose_input.derive_image_id),
                bytemuck::cast_slice(&derive_journal),
            )
            .expect("Failed to lift derivation receipt");
            // todo Verify inclusion of ethereum tail in Merkle root
            // Create output
            ComposeOutput {
                derive_image_id: compose_input.derive_image_id,
                compose_image_id: compose_input.compose_image_id,
                op_head: derive_output.op_head,
                op_tail: derive_output.derived_op_blocks.last().unwrap_or(&derive_output.op_head).clone(),
                eth_tail: derive_output.eth_tail,
                eth_chain_root: compose_input.eth_chain_root
            }
        }
        ComposeInputOperation::JOIN { left: left_compose_output, right: right_compose_output } => {
            // Verify composition receipts
            let left_compose_journal =
                to_vec(&left_compose_output).expect("Failed to encode expected left composition journal");
            env::verify(
                Digest::from(compose_input.compose_image_id),
                bytemuck::cast_slice(&left_compose_journal),
            ).expect("Failed to verify left composition receipt");
            let right_compose_journal =
                to_vec(&right_compose_output).expect("Failed to encode expected right composition journal");
            env::verify(
                Digest::from(compose_input.compose_image_id),
                bytemuck::cast_slice(&right_compose_journal),
            ).expect("Failed to verify right composition receipt");
            // Verify composition continuity
            // derive_image_id equality
            assert_eq!(compose_input.derive_image_id, left_compose_output.derive_image_id);
            assert_eq!(compose_input.derive_image_id, right_compose_output.derive_image_id);
            // compose_image_id equality
            assert_eq!(compose_input.compose_image_id, left_compose_output.compose_image_id);
            assert_eq!(compose_input.compose_image_id, right_compose_output.compose_image_id);
            // eth_chain_root equality
            assert_eq!(compose_input.eth_chain_root, left_compose_output.eth_chain_root);
            assert_eq!(compose_input.eth_chain_root, right_compose_output.eth_chain_root);
            // op block continuity
            assert_eq!(left_compose_output.op_tail, right_compose_output.op_head);

            ComposeOutput {
                derive_image_id: compose_input.derive_image_id,
                compose_image_id: compose_input.compose_image_id,
                op_head: left_compose_output.op_head,
                op_tail: right_compose_output.op_tail,
                eth_tail: core::cmp::max(left_compose_output.eth_tail, right_compose_output.eth_tail),
                eth_chain_root: compose_input.eth_chain_root,
            }
        }
    };
    // output statement about larger segment
    env::commit(&compose_output);
}
