use core::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use anyhow::{anyhow, bail, ensure, Context, Result};
use ethers_core::types::{Transaction, U64, U256 as EU256};
use once_cell::unsync::Lazy;
use zeth_primitives::{ethers::{from_ethers_h160, from_ethers_u256}, transactions::{ethereum::EthereumTxEssence, EthereumTransaction}};

use crate::input::Input;

pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
pub const MAX_TX_LIST: usize = 79;
pub const MAX_TX_LIST_BYTES: usize = 120_000;
pub const BLOCK_GAS_LIMIT: Lazy<U256> = Lazy::new(|| uint!(15250000_U256));
pub const GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});

macro_rules! taiko_contracts {
    ($name:ident) => {{
        use crate::taiko_utils::$name::*;
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
        #[allow(clippy::needless_return)]
        _ => bail!("invalid chain name: {name}"),
    }
}

pub mod testnet {
    use super::*;
    pub const CHAIN_ID: u64 = 167008;
    pub const L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xB20BB9105e007Bd3E0F73d63D4D3dA2c8f736b77")
            .expect("invalid l1 contract address")
    });

    pub const L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x08a3f537c4bbe8B6176420f4Cd0C84b02172dC65")
            .expect("invalid l1 signal service")
    });
    pub const L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670080000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });

    pub const L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670080000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

pub mod internal_devnet_a {
    use super::*;
    pub const CHAIN_ID: u64 = 167001;
    pub const L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0xbE71D121291517c85Ab4d3ac65d70F6b1FD57118")
            .expect("invalid l1 contract address")
    });
    pub const L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x83e383dec6E3C2CD167E3bF6aA8c36F0e55Ad910")
            .expect("invalid l1 signal service")
    });

    pub const L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });

    pub const L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670010000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

pub mod internal_devnet_b {
    use super::*;
    pub const CHAIN_ID: u64 = 167002;
    pub const L1_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x674313F932cc0cE272154a288cf3De474D44e14F")
            .expect("invalid l1 contract address")
    });
    pub const L1_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x83e383dec6E3C2CD167E3bF6aA8c36F0e55Ad910")
            .expect("invalid l1 signal service")
    });

    pub const L2_CONTRACT: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670020000000000000000000000000000010001")
            .expect("invalid l2 contract address")
    });
    pub const L2_SIGNAL_SERVICE: Lazy<Address> = Lazy::new(|| {
        Address::from_str("0x1670020000000000000000000000000000000005")
            .expect("invalid l2 signal service")
    });
}

const GX1: Lazy<U256> =
    Lazy::new(|| uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256));
const N: Lazy<U256> =
    Lazy::new(|| uint!(0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141_U256));
const GX1_MUL_PRIVATEKEY: Lazy<U256> =
    Lazy::new(|| uint!(0x4341adf5a780b4a87939938fd7a032f6e6664c7da553c121d3b4947429639122_U256));
const GX2: Lazy<U256> =
    Lazy::new(|| uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256));

/// check the anchor signature with fixed K value
fn check_anchor_signature(anchor: &EthereumTransaction) -> Result<()> {
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
                "r == GX2, but N != msg_hash + *GX1_MUL_PRIVATEKEY, N: {}, msg_hash: {msg_hash}, *GX1_MUL_PRIVATEKEY: {}",
                *N, *GX1_MUL_PRIVATEKEY
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

pub fn check_anchor_tx(
    input: Input<EthereumTxEssence>,
    anchor: &Transaction,
    chain_name: &str,
) -> Result<()> {
    let tx1559_type = U64::from(0x2);
    ensure!(
        anchor.transaction_type == Some(tx1559_type),
        "anchor transaction type mismatch"
    );

    let tx: EthereumTransaction = anchor
        .clone()
        .try_into()
        .context(anyhow!("failed to decode anchor transaction: {:?}", anchor))?;
    check_anchor_signature(&tx).context(anyhow!("failed to check anchor signature"))?;

    ensure!(
        from_ethers_h160(anchor.from) == *GOLDEN_TOUCH_ACCOUNT,
        "anchor transaction from mismatch"
    );
    ensure!(
        from_ethers_h160(anchor.to.unwrap()) == get_contracts(chain_name).unwrap().0,
        "anchor transaction to mismatch"
    );
    ensure!(
        anchor.value == EU256::from(0),
        "anchor transaction value mismatch"
    );
    ensure!(
        anchor.gas == EU256::from(ANCHOR_GAS_LIMIT),
        "anchor transaction gas price mismatch"
    );
    ensure!(
        from_ethers_u256(anchor.max_fee_per_gas.unwrap()) == input.base_fee_per_gas,
        "anchor transaction gas mismatch"
    );

    //TODO(Brecht)
    // 1. check l2 parent gas used
    /*ensure!(
        l2_parent_block.gas_used == ethers_core::types::U256::from(anchor_call.parentGasUsed),
        "parentGasUsed mismatch"
    );

    // 2. check l1 signal root
    let Some(l1_signal_service) = tp.l1_signal_service else {
        bail!("l1_signal_service not set");
    };

    let proof = tp.l1_provider.get_proof(&ProofQuery {
        block_no: l1_block_no,
        address: l1_signal_service.into_array().into(),
        indices: Default::default(),
    })?;

    let l1_signal_root = from_ethers_h256(proof.storage_hash);

    ensure!(
        l1_signal_root == anchor_call.l1SignalRoot,
        "l1SignalRoot mismatch"
    );

    // 3. check l1 block hash
    ensure!(
        l1_block.hash.unwrap() == ethers_core::types::H256::from(anchor_call.l1Hash.0),
        "l1Hash mismatch"
    );*/

    Ok(())
}
