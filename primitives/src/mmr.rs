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

pub type Hash = [u8; 32];
pub type SiblingMap = HashMap<Hash, Hash>;

/// Returns the hash of two sibling nodes by appending them together while placing
/// the lexicographically smaller node first.
#[inline]
fn branch_hash(a: &Hash, b: &Hash) -> Hash {
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MerkleMountainRange(pub Vec<Option<Hash>>);

impl MerkleMountainRange {
    /// Appends a new leaf to the mountain range, bubbling up all the changes required to
    /// the ancestor tree roots. The optional `_sibling_map` parameter can be
    /// specified in order to record all visited siblings.
    pub fn append_leaf(&mut self, mut value: Hash, mut sibling_map: Option<&mut SiblingMap>) {
        for node in self.0.iter_mut() {
            if node.is_none() {
                node.replace(value);
                return;
            } else {
                let sibling = node.take().unwrap();
                if let Some(sibling_map) = sibling_map.as_mut() {
                    sibling_map.insert(value, sibling);
                    sibling_map.insert(sibling, value);
                }
                value = branch_hash(&value, &sibling)
            }
        }
        self.0.push(Some(value));
    }

    /// Returns the root of the (unbalanced) Merkle tree that covers all the present range
    /// roots. The optional `_sibling_map` parameter can be specified in order to
    /// record all visited siblings.
    pub fn root(&self, mut sibling_map: Option<&mut SiblingMap>) -> Option<Hash> {
        let mut result: Option<Hash> = None;
        for root in self.0.iter().flatten() {
            if let Some(sibling) = result {
                if let Some(sibling_map) = sibling_map.as_mut() {
                    sibling_map.insert(*root, sibling);
                    sibling_map.insert(sibling, *root);
                }
                result.replace(branch_hash(&sibling, root));
            } else {
                result.replace(*root);
            }
        }
        result
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MerkleProof(Vec<Hash>);

impl MerkleProof {
    /// Returns the inclusion proof of the input `value` using the provided `sibling_map`.
    pub fn new(sibling_map: &SiblingMap, mut value: Hash) -> Self {
        let mut roots = vec![value];
        while let Some(sibling) = sibling_map.get(&value) {
            roots.push(*sibling);
            value = branch_hash(sibling, &value);
        }
        Self(roots)
    }

    /// Verifies the inclusion proof against the provided `root` of a
    /// [MerkleMountainRange] and a `value`.
    pub fn verify(&self, root: &Hash, value: &Hash) -> bool {
        let mut iter = self.0.iter();
        match iter.next() {
            None => false,
            Some(first) => {
                first == value && iter.fold(*first, |result, e| branch_hash(&result, e)) == *root
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_proof_verify() {
        let values = vec![[0u8; 32], [1; 32], [2; 32], [3; 32], [4; 32]];
        let value = values[0];
        let mut mmr = MerkleMountainRange::default();
        let mut sibling_map = SiblingMap::new();
        for value in values {
            mmr.append_leaf(value, Some(&mut sibling_map));
        }
        let root = mmr.root(Some(&mut sibling_map)).unwrap();
        let proof = MerkleProof::new(&sibling_map, value);
        assert!(proof.verify(&root, &value));
        // test that the proof is not valid for a different value/root
        assert!(!proof.verify(&root, &[0xff; 32]));
        assert!(!proof.verify(&[0x00; 32], &value));
    }
}
