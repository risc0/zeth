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

extern crate core;

pub mod access_list;
pub mod block;
pub mod keccak;
pub mod receipt;
pub mod transactions;
pub mod trie;
pub mod withdrawal;

#[cfg(feature = "alloy")]
pub mod alloy;

pub mod batch;
pub mod mmr;
#[cfg(feature = "revm")]
pub mod revm;

pub use alloy_primitives::*;
pub use alloy_rlp;

pub trait RlpBytes: Sized {
    /// Decodes the blob into the appropriate type.
    /// The input must contain exactly one value and no trailing data.
    fn decode_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, alloy_rlp::Error>;
}

impl<T> RlpBytes for T
where
    T: alloy_rlp::Decodable,
{
    #[inline]
    fn decode_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, alloy_rlp::Error> {
        let mut buf = bytes.as_ref();
        let this = T::decode(&mut buf)?;
        if buf.is_empty() {
            Ok(this)
        } else {
            Err(alloy_rlp::Error::Custom("Trailing data"))
        }
    }
}
