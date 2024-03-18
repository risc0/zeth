use std::error::Error;

use k256::{ecdsa::RecoveryId, elliptic_curve::PublicKey};
use revm_primitives::{Address, B256, U256};

use crate::keccak::keccak;

/// The order of the secp256k1 curve, divided by two. Signatures that should be checked
/// according to EIP-2 should have an S value less than or equal to this.
///
/// `57896044618658097711785492504343953926418782139537452191302581570759080747168`
const SECP256K1N_HALF: U256 = U256::from_be_bytes([
    0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D, 0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
]);

// Recovers the address of the sender using secp256k1 pubkey recovery.
//
// Converts the public key into an ethereum address by hashing the public key with
// keccak256.
//
// This does not ensure that the `s` value in the signature is low, and _just_ wraps the
// underlying secp256k1 library.
// pub fn recover_signer_unchecked_crypto(sig: &[u8; 65], msg: &[u8; 32]) ->
// Result<Address, Error> { #[cfg(target_os = "zkvm")]
// {
// let pubkey = sp1_precompiles::secp256k1::ecrecover(sig, msg).unwrap();
// return Ok(public_key_bytes_to_address(&pubkey));
// }
// {
// let recid = RecoveryId::from_byte(sig[64]).expect("recovery ID is valid");
// let sig = K256Signature::from_slice(&sig.as_slice()[..64])?;
// let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &sig, recid)?;
// let pubkey = PublicKey::from(&recovered_key);
// Ok(public_key_to_address(pubkey))
// }
// }
//
// Recover signer from message hash, _without ensuring that the signature has a low `s`
// value_.
//
// Using this for signature validation will succeed, even if the signature is malleable or
// not compliant with EIP-2. This is provided for compatibility with old signatures which
// have large `s` values.
// pub fn recover_signer_unchecked(&self, hash: B256) -> Option<Address> {
// let mut sig: [u8; 65] = [0; 65];
//
// sig[0..32].copy_from_slice(&self.r.to_be_bytes::<32>());
// sig[32..64].copy_from_slice(&self.s.to_be_bytes::<32>());
// sig[64] = self.odd_y_parity as u8;
//
// NOTE: we are removing error from underlying crypto library as it will restrain
// primitive errors and we care only if recovery is passing or not.
// recover_signer_unchecked_crypto(&sig, &hash.0).ok()
// }
//
// Recover signer address from message hash. This ensures that the signature S value is
// greater than `secp256k1n / 2`, as specified in
// [EIP-2](https://eips.ethereum.org/EIPS/eip-2).
//
// If the S value is too large, then this will return `None`
// pub fn recover_signer(&self, hash: B256) -> Option<Address> {
// if self.s > SECP256K1N_HALF {
// return None
// }
//
// self.recover_signer_unchecked(hash)
// }
//
//
// Converts a public key into an ethereum address by hashing the encoded public key with
// keccak256.
// pub fn public_key_to_address(public: PublicKey) -> Address {
// let pubkey_bytes =
// public.to_encoded_point(false).as_bytes().try_into().expect("The slice has 65 bytes");
// public_key_bytes_to_address(&pubkey_bytes)
// strip out the first byte because that should be the SECP256K1_TAG_PUBKEY_UNCOMPRESSED
// tag returned by libsecp's uncompressed pubkey serialization
// let hash = keccak256(&public.serialize_uncompressed()[1..]);
// Address::from_slice(&hash[12..])
// }
//
// fn public_key_bytes_to_address(public: &[u8; 65]) -> Address {
// Strip out first byte of sec1 encoded pubkey
// let hash = keccak(&public[1..]);
// Address::from_slice(&hash[12..])
// }
//
