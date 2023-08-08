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

use alloy_primitives::{B160, U256};
use alloy_rlp_derive::{RlpEncodable, RlpMaxEncodedLen};
use anyhow::Context;
use k256::{
    ecdsa::{RecoveryId, Signature as K256Signature, VerifyingKey as K256VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey as K256PublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{
    keccak::keccak,
    transaction::{Transaction, TxEssence, TxEssenceLegacy},
};

/// A signature that can be used to recover the signing public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, RlpMaxEncodedLen)]
pub struct TxSignature {
    pub v: u64,
    pub r: U256,
    pub s: U256,
}

impl Transaction {
    /// Recover the sending party of the transaction.
    pub fn recover_from(&self) -> anyhow::Result<B160> {
        let is_y_odd = self.is_y_odd().context("v invalid")?;
        let signature = K256Signature::from_scalars(
            self.signature.r.to_be_bytes(),
            self.signature.s.to_be_bytes(),
        )
        .context("r, s invalid")?;

        let verify_key = K256VerifyingKey::recover_from_prehash(
            self.essence.signing_hash().as_slice(),
            &signature,
            RecoveryId::new(is_y_odd, false),
        )
        .context("invalid signature")?;

        let public_key = K256PublicKey::from(&verify_key);
        let public_key = public_key.to_encoded_point(false);
        let public_key = public_key.as_bytes();
        debug_assert_eq!(public_key[0], 0x04);
        let hash = keccak(&public_key[1..]);

        Ok(B160::from_slice(&hash[12..]))
    }

    fn is_y_odd(&self) -> Option<bool> {
        match &self.essence {
            TxEssence::Legacy(TxEssenceLegacy { chain_id: None, .. }) => {
                checked_bool(self.signature.v - 27)
            }
            TxEssence::Legacy(TxEssenceLegacy {
                chain_id: Some(chain_id),
                ..
            }) => checked_bool(self.signature.v - 35 - 2 * chain_id),
            _ => checked_bool(self.signature.v),
        }
    }
}

#[inline]
fn checked_bool(v: u64) -> Option<bool> {
    match v {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}
