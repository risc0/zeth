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

use reth_optimism_chainspec::{OpChainSpec, OP_MAINNET};
use reth_primitives::{Block, Header};
use std::sync::Arc;
use zeth_core::db::MemoryDB;
use zeth_core::stateless::driver::RethDriver;
use zeth_core_optimism::{
    OpRethExecutionStrategy, OpRethStatelessClient, OpRethValidationStrategy,
};
use zeth_preflight::client::PreflightClient;
use zeth_preflight::BlockBuilder;

pub struct OpRethBlockBuilder;

impl BlockBuilder<OpChainSpec, Block, Header, MemoryDB, RethDriver> for OpRethBlockBuilder {
    type PreflightClient = OpRethPreflightClient;
    type StatelessClient = OpRethStatelessClient;

    fn chain_spec() -> Arc<OpChainSpec> {
        OP_MAINNET.clone()
    }
}

pub struct OpRethPreflightClient;

impl PreflightClient<OpChainSpec, Block, Header, RethDriver> for OpRethPreflightClient {
    type Validation = OpRethValidationStrategy;
    type Execution = OpRethExecutionStrategy;
}
