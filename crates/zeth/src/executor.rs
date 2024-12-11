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
use crate::profile_file_name;
use risc0_zkvm::ExecutorEnv;
use zeth_preflight::Witness;

pub fn build_executor_env<'b>(
    cli: &Cli,
    witness: &'b Witness,
    image_id: [u32; 8],
    network_name: &str,
) -> anyhow::Result<ExecutorEnv<'b>> {
    let run_args = cli.run_args();
    let mut builder = ExecutorEnv::builder();
    builder.write_frame(&witness.encoded_input);
    builder.segment_limit_po2(run_args.execution_po2);
    if run_args.profile {
        if std::env::var("RISC0_PPROF_OUT").is_ok() {
            log::warn!("Ignoring RISC0_PPROF_OUT because profiling is enabled through CLI.");
        }
        let file_name = profile_file_name(
            network_name,
            witness.chain,
            witness.validated_tail_number,
            witness.validated_tip_number,
            image_id,
        );
        builder.enable_profiler(file_name);
    }
    builder.build()
}
