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

extern crate core;

#[cfg(not(target_os = "zkvm"))]
pub mod host;

pub mod block_builder;
pub mod consts;
pub mod execution;
pub mod finalization;
pub mod initialization;
pub mod input;
pub mod mem_db;
pub mod preparation;

pub use zeth_primitives::transactions::ethereum::EthereumTxEssence;

/// call forget only if running inside the guest
pub fn guest_mem_forget<T>(_t: T) {
    #[cfg(target_os = "zkvm")]
    core::mem::forget(_t)
}

/// Pass-through hashing struct for using pre-hashed keys as HashMap indices
#[derive(Default)]
pub struct NoHasher {
    buf: [u8; 8],
}

/// Takes the first 64 bits as the hash
impl core::hash::Hasher for NoHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        u64::from_be_bytes(self.buf)
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        self.buf.copy_from_slice(&bytes[..8]);
    }
}

#[derive(Copy, Clone, Default)]
pub struct NoHashBuilder {}

impl core::hash::BuildHasher for NoHashBuilder {
    type Hasher = NoHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        Default::default()
    }
}
