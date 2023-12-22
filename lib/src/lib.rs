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
#![feature(path_file_prefix)]
#![cfg_attr(target_os = "zkvm", no_std)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
extern crate core;

pub mod block_builder;
pub mod consts;
pub mod input;
pub mod mem_db;
pub mod preparation;

#[cfg(feature = "taiko")]
pub mod taiko;

#[cfg(feature = "std")]
#[cfg(not(target_os = "zkvm"))]
pub mod host;

mod utils;

pub use zeth_primitives::transactions::{ethereum::EthereumTxEssence, optimism::OptimismTxEssence};

/// call forget only if running inside the guest
pub fn guest_mem_forget<T>(_t: T) {
    #[cfg(target_os = "zkvm")]
    core::mem::forget(_t)
}
