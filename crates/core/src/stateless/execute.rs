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

use crate::driver::CoreDriver;
use alloy_primitives::U256;
use k256::ecdsa::VerifyingKey;
use reth_revm::db::BundleState;
use std::sync::Arc;

pub trait ExecutionStrategy<Driver: CoreDriver, Database> {
    fn execute_transactions(
        chain_spec: Arc<Driver::ChainSpec>,
        block: &mut Driver::Block,
        signers: &[VerifyingKey],
        total_difficulty: &mut U256,
        db: &mut Option<Database>,
    ) -> anyhow::Result<BundleState>;
}
