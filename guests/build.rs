// Copyright 2023, 2024 RISC Zero, Inc.
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

#[cfg(not(any(feature = "debug-guest-build", debug_assertions)))]
fn main() {
    let cwd = std::env::current_dir().unwrap();
    let root_dir = cwd.parent().map(|d| d.to_path_buf());
    let build_opts = std::collections::HashMap::from_iter(
        ["zeth-guests-reth-ethereum", "zeth-guests-reth-optimism"]
            .into_iter()
            .map(|guest_pkg| {
                (
                    guest_pkg,
                    risc0_build::GuestOptions {
                        features: vec![],
                        use_docker: Some(risc0_build::DockerOptions {
                            root_dir: root_dir.clone(),
                        }),
                    },
                )
            }),
    );
    risc0_build::embed_methods_with_options(build_opts);
}

#[cfg(any(feature = "debug-guest-build", debug_assertions))]
fn main() {
    risc0_build::embed_methods();
}
