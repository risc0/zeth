use alloy_sol_types::{sol, SolCall};
use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::Lazy;

use crate::{transactions::EthereumTransaction, uint, U256};

static GX1: Lazy<U256> =
    Lazy::new(|| uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256));
static N: Lazy<U256> =
    Lazy::new(|| uint!(0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141_U256));
static GX1_MUL_PRIVATEKEY: Lazy<U256> =
    Lazy::new(|| uint!(0x4341adf5a780b4a87939938fd7a032f6e6664c7da553c121d3b4947429639122_U256));
static GX2: Lazy<U256> =
    Lazy::new(|| uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256));

sol! {
    function anchor(
        bytes32 l1Hash,
        bytes32 l1SignalRoot,
        uint64 l1Height,
        uint32 parentGasUsed
    )
        external
    {}
}

/// decode anchor arguments from anchor transaction
pub fn decode_anchor_call_args(data: &[u8]) -> Result<anchorCall> {
    let anchor_call =
        anchorCall::abi_decode(data, false).with_context(|| "failed to decode anchor call")?;
    Ok(anchor_call)
}

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
        // alias: when r == GX2 require N == msg_hash + GX1_MUL_PRIVATEKEY
        if *N != msg_hash + *GX1_MUL_PRIVATEKEY {
            bail!(
                "r == GX2, but N != msg_hash + GX1_MUL_PRIVATEKEY, N: {}, msg_hash: {}, GX1_MUL_PRIVATEKEY: {}",
                *N, msg_hash, *GX1_MUL_PRIVATEKEY
            );
        }
        return Ok(());
    }
    Err(anyhow!(
        "r != GX1 && r != GX2, r: {}, GX1: {}, GX2: {}",
        sign.r,
        *GX1,
        *GX2
    ))
}
