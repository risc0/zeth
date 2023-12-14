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

#[cfg(target_os = "zkvm")]
use risc0_zkvm::{guest::env, serde::to_vec, sha::Digest};
use serde::{Deserialize, Serialize};
use zeth_primitives::{block::Header, tree::MerkleMountainRange, BlockHash, BlockNumber};

use crate::optimism::DeriveOutput;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ComposeInput {
    pub derive_image_id: [u32; 8],
    pub compose_image_id: [u32; 8],
    pub operation: ComposeInputOperation,
    pub eth_chain_root: [u8; 32],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ComposeInputOperation {
    PREP {
        eth_blocks: Vec<Header>,
        mountain_range: MerkleMountainRange,
        prior: Option<ComposeOutput>,
    },
    LIFT {
        derivation: DeriveOutput,
        eth_tail_proof: MerkleMountainRange,
    },
    JOIN {
        left: ComposeOutput,
        right: ComposeOutput,
    },
    FINISH {
        prep: ComposeOutput,
        aggregate: ComposeOutput,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub struct ComposeOutput {
    pub derive_image_id: [u32; 8],
    pub compose_image_id: [u32; 8],
    pub operation: ComposeOutputOperation,
    pub eth_chain_root: [u8; 32],
    pub eth_chain_root_validated: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub enum ComposeOutputOperation {
    PREP {
        eth_tail: BlockHash,
    },
    AGGREGATE {
        op_head: (BlockNumber, BlockHash),
        op_tail: (BlockNumber, BlockHash),
        eth_tail: (BlockNumber, BlockHash),
    },
}

impl ComposeInput {
    pub fn process(self) -> ComposeOutput {
        match self.operation {
            ComposeInputOperation::PREP {
                eth_blocks,
                mountain_range,
                prior,
            } => {
                // Check initial data
                let (mut eth_tail, mut mountain_range) = if let Some(prior_output) = prior {
                    #[cfg(target_os = "zkvm")]
                    {
                        // A valid receipt should be provided for prior aggregation
                        let compose_journal = to_vec(&prior_output)
                            .expect("Failed to encode prior aggregation journal");
                        env::verify(
                            Digest::from(self.compose_image_id),
                            bytemuck::cast_slice(&compose_journal),
                        )
                        .expect("Failed to validate prior aggregation");
                    }
                    // Validate context
                    assert_eq!(self.derive_image_id, prior_output.derive_image_id);
                    assert_eq!(self.compose_image_id, prior_output.compose_image_id);
                    assert_eq!(self.eth_chain_root, prior_output.eth_chain_root);
                    // Only append merkle range from preparation outputs
                    let ComposeOutputOperation::PREP { eth_tail } = prior_output.operation else {
                        unimplemented!()
                    };

                    // Root of input mountain range should equal prior prep's root
                    assert_eq!(
                        mountain_range
                            .root()
                            .expect("Empty mountain range used as input"),
                        self.eth_chain_root
                    );

                    (Some(eth_tail), mountain_range)
                } else {
                    Default::default()
                };
                // Insert chain of blocks into mountain range
                for block in eth_blocks {
                    // Validate parent relationship
                    if let Some(parent_hash) = eth_tail {
                        assert_eq!(block.parent_hash, parent_hash);
                    }
                    // Derive block's keccak hash
                    let block_hash = block.hash();
                    // Insert hash into mountain range
                    mountain_range.append_leaf(block_hash.0);
                    // Mark block as new tail
                    eth_tail.replace(block_hash);
                }

                ComposeOutput {
                    derive_image_id: self.derive_image_id,
                    compose_image_id: self.compose_image_id,
                    operation: ComposeOutputOperation::PREP {
                        eth_tail: eth_tail.expect("No blocks used for preparation"),
                    },
                    eth_chain_root: mountain_range.root().expect("Created empty range"),
                    eth_chain_root_validated: true,
                }
            }
            ComposeInputOperation::LIFT {
                derivation: derive_output,
                eth_tail_proof,
            } => {
                #[cfg(target_os = "zkvm")]
                {
                    // Verify derivation receipt
                    let derive_journal = to_vec(&derive_output)
                        .expect("Failed to encode expected derivation journal");
                    env::verify(
                        Digest::from(self.derive_image_id),
                        bytemuck::cast_slice(&derive_journal),
                    )
                    .expect("Failed to lift derivation receipt");
                }
                // Verify inclusion of ethereum tail in Merkle root
                assert_eq!(
                    self.eth_chain_root,
                    eth_tail_proof
                        .root()
                        .expect("No proof included for ethereum tail")
                );
                assert_eq!(
                    eth_tail_proof.roots.first().unwrap().unwrap(),
                    derive_output.eth_tail.1 .0
                );
                // Create output
                ComposeOutput {
                    derive_image_id: self.derive_image_id,
                    compose_image_id: self.compose_image_id,
                    operation: ComposeOutputOperation::AGGREGATE {
                        op_head: derive_output.op_head,
                        op_tail: derive_output
                            .derived_op_blocks
                            .last()
                            .unwrap_or(&derive_output.op_head)
                            .clone(),
                        eth_tail: derive_output.eth_tail,
                    },
                    eth_chain_root: self.eth_chain_root,
                    eth_chain_root_validated: false,
                }
            }
            ComposeInputOperation::JOIN {
                left: left_compose_output,
                right: right_compose_output,
            } => {
                #[cfg(target_os = "zkvm")]
                {
                    // Verify composition receipts
                    let left_compose_journal = to_vec(&left_compose_output)
                        .expect("Failed to encode expected left composition journal");
                    env::verify(
                        Digest::from(self.compose_image_id),
                        bytemuck::cast_slice(&left_compose_journal),
                    )
                    .expect("Failed to verify left composition receipt");
                    let right_compose_journal = to_vec(&right_compose_output)
                        .expect("Failed to encode expected right composition journal");
                    env::verify(
                        Digest::from(self.compose_image_id),
                        bytemuck::cast_slice(&right_compose_journal),
                    )
                    .expect("Failed to verify right composition receipt");
                }
                // Validate context
                // derive_image_id equality
                assert_eq!(self.derive_image_id, left_compose_output.derive_image_id);
                assert_eq!(self.derive_image_id, right_compose_output.derive_image_id);
                // compose_image_id equality
                assert_eq!(self.compose_image_id, left_compose_output.compose_image_id);
                assert_eq!(self.compose_image_id, right_compose_output.compose_image_id);
                // eth_chain_root equality
                assert_eq!(self.eth_chain_root, left_compose_output.eth_chain_root);
                assert_eq!(self.eth_chain_root, right_compose_output.eth_chain_root);

                // Verify op block continuity
                let ComposeOutputOperation::AGGREGATE {
                    op_head: left_op_head,
                    op_tail: left_op_tail,
                    eth_tail: left_eth_tail,
                } = left_compose_output.operation
                else {
                    unimplemented!()
                };
                let ComposeOutputOperation::AGGREGATE {
                    op_head: right_op_head,
                    op_tail: right_op_tail,
                    eth_tail: right_eth_tail,
                } = right_compose_output.operation
                else {
                    unimplemented!()
                };
                assert_eq!(&left_op_tail, &right_op_head);

                ComposeOutput {
                    derive_image_id: self.derive_image_id,
                    compose_image_id: self.compose_image_id,
                    operation: ComposeOutputOperation::AGGREGATE {
                        op_head: left_op_head,
                        op_tail: right_op_tail,
                        eth_tail: core::cmp::max(left_eth_tail, right_eth_tail),
                    },
                    eth_chain_root: self.eth_chain_root,
                    eth_chain_root_validated: left_compose_output.eth_chain_root_validated
                        || right_compose_output.eth_chain_root_validated,
                }
            }
            ComposeInputOperation::FINISH { prep, aggregate } => {
                // Verify prep receipt
                #[cfg(target_os = "zkvm")]
                {
                    // A valid receipt should be provided for prior aggregation
                    let compose_journal = to_vec(&prep).expect("Failed to encode prep journal");
                    env::verify(
                        Digest::from(self.compose_image_id),
                        bytemuck::cast_slice(&compose_journal),
                    )
                    .expect("Failed to validate prep receipt");
                }
                // Verify aggregate receipt
                #[cfg(target_os = "zkvm")]
                {
                    // A valid receipt should be provided for prior aggregation
                    let compose_journal =
                        to_vec(&aggregate).expect("Failed to encode prep journal");
                    env::verify(
                        Digest::from(self.compose_image_id),
                        bytemuck::cast_slice(&compose_journal),
                    )
                    .expect("Failed to validate aggregate receipt");
                }
                // Validate context
                // derive_image_id equality
                assert_eq!(self.derive_image_id, prep.derive_image_id);
                assert_eq!(self.derive_image_id, aggregate.derive_image_id);
                // compose_image_id equality
                assert_eq!(self.compose_image_id, prep.compose_image_id);
                assert_eq!(self.compose_image_id, aggregate.compose_image_id);
                // eth_chain_root equality
                assert_eq!(self.eth_chain_root, prep.eth_chain_root);
                assert_eq!(self.eth_chain_root, aggregate.eth_chain_root);
                // Output new aggregate with validated chain root
                ComposeOutput {
                    derive_image_id: self.derive_image_id,
                    compose_image_id: self.compose_image_id,
                    operation: aggregate.operation,
                    eth_chain_root: self.eth_chain_root,
                    eth_chain_root_validated: true,
                }
            }
        }
    }
}
