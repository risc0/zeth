pub mod anchor;
pub mod consts;
pub mod proposal;
pub mod protocol_instance;
pub mod utils;

pub use anchor::*;
pub use consts::*;
pub use proposal::*;
pub use protocol_instance::*;
use thiserror_no_std::Error as ThisError;
use alloy_primitives::U256;
use alloy_rlp_derive::{RlpEncodable, RlpMaxEncodedLen};
use serde::{Deserialize, Serialize};
pub use utils::*;

#[derive(ThisError, Debug)]
#[error(transparent)]
struct AbiEncodeError(#[from] alloy_sol_types::Error);

/// Represents a cryptographic signature associated with a transaction.
///
/// The `TxSignature` struct encapsulates the components of an ECDSA signature: `v`, `r`,
/// and `s`. This signature can be used to recover the public key of the signer, ensuring
/// the authenticity of the transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, RlpMaxEncodedLen)]
pub struct TxSignature {
    pub v: u64,
    pub r: U256,
    pub s: U256,
}

impl TxSignature {
    /// Computes the length of the RLP-encoded signature payload in bytes.
    pub fn payload_length(&self) -> usize {
        self._alloy_rlp_payload_length()
    }

    pub fn to_bytes(&self) -> [u8; 65] {
        let mut sig = [0u8; 65];
        sig[..32].copy_from_slice(&self.r.to_be_bytes::<32>());
        sig[32..64].copy_from_slice(&self.s.to_be_bytes::<32>());
        sig[64] = (self.v + 27) as u8;
        sig
    }
}
