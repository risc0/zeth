// Copyright 2025 RISC Zero, Inc.
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

use risc0_build::{DockerOptionsBuilder, GuestOptionsBuilder};
use std::{collections::HashMap, env, path::PathBuf};

fn main() {
    // This build script is responsible for building the guest code and embedding the resulting
    // ELF binaries into the host crate.

    println!("cargo:rerun-if-env-changed=RISC0_USE_DOCKER");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.parent().unwrap();

    let mut guest_opts = GuestOptionsBuilder::default();

    // Use Docker for deterministic builds if RISC0_USE_DOCKER is set.
    if env::var("RISC0_USE_DOCKER").is_ok() {
        // Get the active rustc version to create a version-specific Docker tag.
        let rust_version = rustc_version::version().expect("failed to get rustc version");
        let docker_tag = format!("r0.{}.{}.0", rust_version.major, rust_version.minor);
        println!("cargo:warning=Using Docker build with tag: {}", &docker_tag);

        let docker_opts = DockerOptionsBuilder::default()
            // Set the root of the Docker build context to the workspace root.
            .root_dir(workspace_root)
            .docker_container_tag(docker_tag)
            .build()
            .expect("failed to build docker options");

        guest_opts.use_docker(docker_opts);
    }

    let guest_options = guest_opts.build().expect("failed to build guest options");

    risc0_build::embed_methods_with_options(HashMap::from([("stateless-client", guest_options)]));
}
