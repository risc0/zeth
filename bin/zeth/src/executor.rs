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

use crate::cli::Cli;
use risc0_zkvm::ExecutorEnv;

pub fn build_executor_env<'b>(cli: &Cli, input: &'b [u8]) -> anyhow::Result<ExecutorEnv<'b>> {
    let run_args = cli.run_args();
    let mut builder = ExecutorEnv::builder();
    builder.write_slice(input);
    builder.segment_limit_po2(run_args.execution_po2);
    builder.build()
}
