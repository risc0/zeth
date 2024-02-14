use core::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use anyhow::{anyhow, bail, Result};
use once_cell::sync::Lazy;
use zeth_primitives::transactions::EthereumTransaction;

pub static ANCHOR_GAS_LIMIT: u64 = 250_000;
pub static MAX_TX_LIST: usize = 79;
pub static MAX_TX_LIST_BYTES: usize = 120_000;
pub static BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(15250000_U256));
pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});

macro_rules! taiko_contracts {
    ($name:ident) => {{
        use crate::taiko::consts::$name::*;
        Ok((
            *L1_CONTRACT,
            *L2_CONTRACT,
            *L1_SIGNAL_SERVICE,
            *L2_SIGNAL_SERVICE,
        ))
    }};
}

pub fn get_contracts(name: &str) -> Result<(Address, Address, Address, Address)> {
    match name {
        "testnet" => taiko_contracts!(testnet),
        "internal_devnet_a" => taiko_contracts!(internal_devnet_a),
        "internal_devnet_b" => taiko_contracts!(internal_devnet_b),
        _ => bail!("invalid chain name: {}", name),
    }
}

pub mod testnet {
    use super::*;
    pub static CHAIN_ID: u64 = 167008;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xB20BB9105e007Bd3E0F73d63D4D3dA2c8f736b77")
            .expect("invalid l1 contract address")
    });

    pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x08a3f537c4bbe8B6176420f4Cd0C84b02172dC65")
            .expect("invalid l1 signal service")
    });
    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670080000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });

    pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670080000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

pub mod internal_devnet_a {
    use super::*;
    pub static CHAIN_ID: u64 = 167001;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xbE71D121291517c85Ab4d3ac65d70F6b1FD57118")
            .expect("invalid l1 contract address")
    });
    pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x83e383dec6E3C2CD167E3bF6aA8c36F0e55Ad910")
            .expect("invalid l1 signal service")
    });

    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });

    pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

pub mod internal_devnet_b {
    use super::*;
    pub static CHAIN_ID: u64 = 167002;
    pub static L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x674313F932cc0cE272154a288cf3De474D44e14F")
            .expect("invalid l1 contract address")
    });
    pub static L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x83e383dec6E3C2CD167E3bF6aA8c36F0e55Ad910")
            .expect("invalid l1 signal service")
    });

    pub static L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670020000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
    pub static L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670020000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

static GX1: Lazy<U256> =
    Lazy::new(|| uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256));
static N: Lazy<U256> =
    Lazy::new(|| uint!(0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141_U256));
static GX1_MUL_PRIVATEKEY: Lazy<U256> =
    Lazy::new(|| uint!(0x4341adf5a780b4a87939938fd7a032f6e6664c7da553c121d3b4947429639122_U256));
static GX2: Lazy<U256> =
    Lazy::new(|| uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256));

/// check the anchor signature with fixed K value
pub fn check_anchor_signature(anchor: &EthereumTransaction) -> Result<()> {
    let sign = &anchor.signature;
    if sign.r == *GX1 {
        return Ok(());
    }
    let msg_hash = anchor.essence.signing_hash();
    let msg_hash: U256 = msg_hash.into();
    if sign.r == *GX2 {
        // when r == GX2 require s == 0 if k == 1
        // alias: when r == GX2 require N == msg_hash + *GX1_MUL_PRIVATEKEY
        if *N != msg_hash + *GX1_MUL_PRIVATEKEY {
            bail!(
                "r == GX2, but N != msg_hash + *GX1_MUL_PRIVATEKEY, N: {}, msg_hash: {}, *GX1_MUL_PRIVATEKEY: {}",
                *N, msg_hash, *GX1_MUL_PRIVATEKEY
            );
        }
        return Ok(());
    }
    Err(anyhow!(
        "r != *GX1 && r != GX2, r: {}, *GX1: {}, GX2: {}",
        sign.r,
        *GX1,
        *GX2
    ))
}
