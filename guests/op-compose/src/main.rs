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
use risc0_zkvm::sha::Digest;
use zeth_lib::optimism::{ComposeInput, ComposeInputOperation, DeriveInput, DeriveMachine, MemDb};

risc0_zkvm::guest::entry!(main);

pub fn main() {
    // read input that specifies two adjacent derivation proofs
    let compose_input: ComposeInput<MemDb> = env::read();

    match compose_input.operation {
        ComposeInputOperation::LIFT(derive_output) => {

        }
        ComposeInputOperation::JOIN { .. } => {}
    }

    // own image id
    // left proof: op_block_head, op_block_tail, eth_block_tail
    // right proof: op_block_head, op_block_tail, eth_block_tail
    // verify left proof & right proof
    // check continuity of optimism blocks
    //  -> right head is parent of left tail
    // check overlap of ethereum blocks
    //  -> derive eth block of right op_block_head
    //  -> check chain of blocks that covers left tail and right tail
    //      -> just check for inclusion proofs for tails under merkle root
    // output statement about larger segment
    //  -> own image id
    env::commit(&compose_input.image_id);
    //  -> op-head of left proof as new op head
    //  -> op-tail of right proof as new op tail
    //  -> left/right eth tail with max height as new tail
    //  -> reference blockchain merkle root
}
