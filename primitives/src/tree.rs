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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MerkleMountainRange {
    pub roots: Vec<Option<[u8; 32]>>,
}

impl MerkleMountainRange {
    /// Appends a new leaf to the mountain range, bubbling up all the changes required to
    /// the ancestor tree roots. The optional `_sibling_map` parameter can be
    /// specified in order to record all visited siblings.
    pub fn append_leaf(
        &mut self,
        mut value: [u8; 32],
        mut _sibling_map: Option<&mut HashMap<[u8; 32], [u8; 32]>>,
    ) {
        for node in self.roots.iter_mut() {
            if node.is_none() {
                node.replace(value);
                return;
            } else {
                let sibling = node.take().unwrap();
                // We only need to log siblings outside the zkVM
                #[cfg(not(target_os = "zkvm"))]
                if let Some(sibling_map) = _sibling_map.as_mut() {
                    sibling_map.insert(value, sibling);
                    sibling_map.insert(sibling, value);
                }
                value = Self::branch_hash(&value, &sibling)
            }
        }
        self.roots.push(Some(value));
    }

    /// Returns the root of the (unbalanced) Merkle tree that covers all the present range
    /// roots. The optional `_sibling_map` parameter can be specified in order to
    /// record all visited siblings.
    pub fn root(
        &self,
        mut _sibling_map: Option<&mut HashMap<[u8; 32], [u8; 32]>>,
    ) -> Option<[u8; 32]> {
        let mut result: Option<[u8; 32]> = None;
        for root in self.roots.iter().flatten() {
            if let Some(sibling) = result {
                // We only need to log siblings outside the zkVM
                #[cfg(not(target_os = "zkvm"))]
                if let Some(sibling_map) = _sibling_map.as_mut() {
                    sibling_map.insert(*root, sibling);
                    sibling_map.insert(sibling, *root);
                }
                result.replace(Self::branch_hash(&sibling, root));
            } else {
                result.replace(*root);
            }
        }
        result
    }

    /// Returns the hash of two sibling nodes by appending them together while placing
    /// the lexicographically smaller node first.
    #[inline]
    fn branch_hash(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        if a < b {
            hasher.update(a);
            hasher.update(b);
        } else {
            hasher.update(b);
            hasher.update(a);
        };
        hasher.finalize().into()
    }

    /// Returns the inclusion proof of the input `value` using the provided `sibling_map`.
    pub fn proof(sibling_map: &HashMap<[u8; 32], [u8; 32]>, mut value: [u8; 32]) -> Self {
        let mut roots = vec![Some(value)];
        while let Some(sibling) = sibling_map.get(&value) {
            roots.push(Some(*sibling));
            value = Self::branch_hash(sibling, &value);
        }
        Self { roots }
    }
}
