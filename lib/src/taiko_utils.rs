use core::str::FromStr;

use alloy_primitives::{uint, Address, U256};
use anyhow::{anyhow, bail, ensure, Context, Result};
use once_cell::unsync::Lazy;
use zeth_primitives::transactions::{
    ethereum::{EthereumTxEssence, TransactionKind},
    EthereumTransaction, TxEssence,
};

use crate::input::{decode_anchor, GuestInput};

pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
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
        Address::from_str("0xC069c3d2a9f2479F559AD34485698ad5199C555f")
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
        Address::from_str("0x1670010000000000000000000000000000010001")
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
    input: &GuestInput<EthereumTxEssence>,
    anchor: &EthereumTransaction,
    from: &Address,
    chain_name: &str,
) -> Result<()> {
    // Check the signature
    check_anchor_signature(anchor).context(anyhow!("failed to check anchor signature"))?;

    // Check the data
    match &anchor.essence {
        EthereumTxEssence::Eip1559(tx) => {
            // Extract the `to` address
            let to = if let TransactionKind::Call(to_addr) = tx.to {
                to_addr
            } else {
                panic!("anchor tx not a smart contract call")
            };
            // Check that it's from the golden touch address
            ensure!(
                *from == GOLDEN_TOUCH_ACCOUNT.clone(),
                "anchor transaction from mismatch"
            );
            // Check that the L2 contract is being called
            ensure!(
                to == get_contracts(chain_name).unwrap().1,
                "anchor transaction to mismatch"
            );
            // Tx can't have any ETH attached
            ensure!(
                tx.value == U256::from(0),
                "anchor transaction value mismatch"
            );
            // Tx needs to have the expected gas limit
            ensure!(
                tx.gas_limit == U256::from(ANCHOR_GAS_LIMIT),
                "anchor transaction gas price mismatch"
            );
            // Check needs to have the base fee set to the block base fee
            ensure!(
                tx.max_fee_per_gas == input.base_fee_per_gas,
                "anchor transaction gas mismatch"
            );

            // Okay now let's decode the anchor tx to verify the inputs
            let anchor_call = decode_anchor(anchor.essence.data())?;

            // TODO(Brecht): somehow l1_header.hash() return the incorrect hash on devnets
            // maybe because those are on cancun but shouldn't have an impact on block hash
            // calculation?
            println!("anchor: {:?}", anchor_call.l1Hash);
            println!("expected: {:?}", input.taiko.l1_header.hash());
            if chain_name == "testnet" {
                // The L1 blockhash needs to match the expected value
                // TODO(Brecht): needs dynamic hash calculation based on L1 block number
                //ensure!(
                //    anchor_call.l1Hash == input.taiko.l1_header.hash(),
                //    "L1 hash mismatch"
                //);
            }
            if chain_name != "testnet" {
                ensure!(
                    anchor_call.l1SignalRoot == input.taiko.l1_header.state_root,
                    "L1 state root mismatch"
                );
            }
            ensure!(
                anchor_call.l1Height == input.taiko.l1_header.number,
                "L1 block number mismatch"
            );
            // The parent gas used input needs to match the gas used value of the parent block
            ensure!(
                U256::from(anchor_call.parentGasUsed) == input.parent_header.gas_used,
                "parentGasUsed mismatch"
            );
        }
        _ => {
            panic!("invalid anchor tx type");
        }
    }

    Ok(())
}
