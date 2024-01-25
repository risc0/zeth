use std::{fs, path::Path};

use rand_core::OsRng;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Error, KeyPair, Message, PublicKey, SecretKey, SECP256K1,
};
use zeth_primitives::{keccak256, signature::TxSignature, Address, B256, U256};

pub fn generate_key() -> KeyPair {
    KeyPair::new_global(&mut OsRng)
}

/// Recovers the address of the sender using secp256k1 pubkey recovery.
///
/// Converts the public key into an ethereum address by hashing the public key with
/// keccak256.
///
/// This does not ensure that the `s` value in the signature is low, and _just_ wraps the
/// underlying secp256k1 library.
#[allow(dead_code)]
pub fn recover_signer_unchecked(sig: &[u8; 65], msg: &[u8; 32]) -> Result<Address, Error> {
    let sig = RecoverableSignature::from_compact(
        &sig[0..64],
        RecoveryId::from_i32(sig[64] as i32 - 27)?,
    )?;

    let public = SECP256K1.recover_ecdsa(&Message::from_slice(&msg[..32])?, &sig)?;
    Ok(public_key_to_address(&public))
}

/// Signs message with the given secret key.
/// Returns the corresponding signature.
pub fn sign_message(secret_key: &SecretKey, message: B256) -> Result<TxSignature, Error> {
    let secret = B256::from_slice(&secret_key.secret_bytes()[..]);
    let sec = SecretKey::from_slice(secret.as_ref())?;
    let s = SECP256K1.sign_ecdsa_recoverable(&Message::from_slice(&message[..])?, &sec);
    let (rec_id, data) = s.serialize_compact();

    let signature = TxSignature {
        r: U256::try_from_be_slice(&data[..32]).expect("The slice has at most 32 bytes"),
        s: U256::try_from_be_slice(&data[32..64]).expect("The slice has at most 32 bytes"),
        v: (rec_id.to_i32() != 0) as u64,
    };
    Ok(signature)
}

/// Converts a public key into an ethereum address by hashing the encoded public key with
/// keccak256.
pub fn public_key_to_address(public: &PublicKey) -> Address {
    // strip out the first byte because that should be the SECP256K1_TAG_PUBKEY_UNCOMPRESSED
    // tag returned by libsecp's uncompressed pubkey serialization
    let hash = keccak256(&public.serialize_uncompressed()[1..]);
    Address::from_slice(&hash[12..])
}

pub fn load_private_key<T: AsRef<Path>>(path: T) -> Result<SecretKey, Error> {
    let data = fs::read(path).unwrap();
    SecretKey::from_slice(data.as_ref())
}

pub fn public_key(secret: &SecretKey) -> PublicKey {
    PublicKey::from_secret_key_global(secret)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    #[test]
    fn recover() {
        let proof = "01000000c13bd882edb37ffbabc9f9e34a0d9789633b850fe55e625b768cc8e5feed7d9f7ab536cbc210c2fcc1385aaf88d8a91d8adc2740245f9deee5fd3d61dd2a71662fb6639515f1e2f3354361a82d86c1952352c1a81b";
        let proof_bytes = hex::decode(proof).unwrap();
        let msg = "216ac5cd5a5e13b0c9a81efb1ad04526b9f4ddd2fe6ebc02819c5097dfb0958c";
        let msg_bytes = hex::decode(msg).unwrap();
        let proof_addr = recover_signer_unchecked(
            &proof_bytes[24..].try_into().unwrap(),
            &msg_bytes.try_into().unwrap(),
        )
        .unwrap();
        let priv_key = "324b5d1744ec27d6ac458350ce6a6248680bb0209521b2c730c1fe82a433eb54";
        let priv_key = SecretKey::from_str(priv_key).unwrap();
        let pubkey = public_key(&priv_key);
        let pub_addr = public_key_to_address(&pubkey);
        assert_eq!(pub_addr, proof_addr);
        println!("Public address: {}", pub_addr);
        println!("Proof public address: {}", proof_addr);
    }
}
