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
use std::sync::Arc;

pub trait ValidationStrategy<Driver: CoreDriver, Database> {
    fn validate_header(
        chain_spec: Arc<Driver::ChainSpec>,
        block: &mut Driver::Block,
        parent_header: &mut Driver::Header,
        total_difficulty: &mut U256,
    ) -> anyhow::Result<()>;
}
