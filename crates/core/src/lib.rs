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

use crate::keccak::keccak;
use alloy_primitives::{Address, Signature, B256};
use k256::ecdsa::signature::hazmat::PrehashVerifier;
use k256::ecdsa::VerifyingKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::PublicKey;

pub mod db;
pub mod driver;
pub mod keccak;
pub mod mpt;
pub mod rescue;
pub mod stateless;

pub fn recover_sender(
    verifying_key: &VerifyingKey,
    signature: Signature,
    transaction_hash: B256,
) -> anyhow::Result<Address> {
    // Verify signature
    let signature = signature.to_k256()?;
    verifying_key.verify_prehash(transaction_hash.as_slice(), &signature)?;
    // Derive wallet address
    let public_key = PublicKey::from(verifying_key).to_encoded_point(false);
    let public_key = public_key.as_bytes();
    let hash = keccak(&public_key[1..]);

    Ok(Address::from_slice(&hash[12..]))
}
