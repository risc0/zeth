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

use std::collections::HashMap;

use alloy_primitives::{b256, B256};
use k256::sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

/// Represents the Keccak-256 hash of an empty byte slice.
///
/// This is a constant value and can be used as a default or placeholder
/// in various cryptographic operations.
pub const SHA256_ZERO: B256 =
    b256!("0000000000000000000000000000000000000000000000000000000000000000");

#[inline]
pub fn sha256(data: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(&data).into()
}

#[inline]
fn branch_hash(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let data = if a < b {
        [a.as_slice(), b.as_slice()].concat()
    } else {
        [b.as_slice(), a.as_slice()].concat()
    };
    sha256(data)
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MerkleMountainRange {
    pub roots: Vec<Option<[u8; 32]>>,
}

impl MerkleMountainRange {
    pub fn append_leaf(&mut self, mut value: [u8; 32]) {
        for node in self.roots.iter_mut() {
            if node.is_none() {
                node.replace(value);
                return;
            } else {
                value = branch_hash(&value, &node.take().unwrap())
            }
        }
        self.roots.push(Some(value));
    }

    pub fn logged_append_leaf(
        &mut self,
        mut value: [u8; 32],
        sibling_map: &mut HashMap<[u8; 32], [u8; 32]>,
    ) {
        for node in self.roots.iter_mut() {
            if node.is_none() {
                node.replace(value);
                return;
            } else {
                let sibling = node.take().unwrap();
                sibling_map.insert(value, sibling);
                sibling_map.insert(sibling, value);
                value = branch_hash(&value, &sibling)
            }
        }
        self.roots.push(Some(value));
    }

    pub fn root(&self) -> Option<[u8; 32]> {
        let mut result: Option<[u8; 32]> = None;
        for root in self.roots.iter().flatten() {
            if let Some(sibling) = result {
                result.replace(branch_hash(&sibling, root));
            } else {
                result.replace(*root);
            }
        }
        result
    }

    pub fn logged_root(&self, sibling_map: &mut HashMap<[u8; 32], [u8; 32]>) -> Option<[u8; 32]> {
        let mut result: Option<[u8; 32]> = None;
        for root in self.roots.iter().flatten() {
            if let Some(sibling) = result {
                sibling_map.insert(*root, sibling);
                sibling_map.insert(sibling, *root);
                result.replace(branch_hash(&sibling, root));
            } else {
                result.replace(*root);
            }
        }
        result
    }

    pub fn proof(sibling_map: &HashMap<[u8; 32], [u8; 32]>, mut value: [u8; 32]) -> Self {
        let mut roots = vec![Some(value)];
        while let Some(sibling) = sibling_map.get(&value) {
            roots.push(Some(*sibling));
            value = branch_hash(sibling, &value);
        }
        Self { roots }
    }
}
