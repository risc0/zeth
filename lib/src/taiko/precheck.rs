use std::error::Error;

use anyhow::{bail, Context, Result};
use ethers_core::types::{Block, Transaction as EthersTransaction, U256 as EthersU256, U64};
use zeth_primitives::{
    ethers::from_ethers_h160,
    taiko::{
        anchor::check_anchor_signature, ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT, MAX_TX_LIST,
        MAX_TX_LIST_BYTES,
    },
    transactions::EthereumTransaction,
    Address,
};

use crate::{
    consts::ChainSpec,
    taiko::{host::TaikoExtra, utils::rlp_decode_list},
};

// rebuild the block with anchor transaction and txlist from l1 contract, then precheck it
pub fn rebuild_and_precheck_block(
    l2_chain_spec: &ChainSpec,
    l2_fini: &mut Block<EthersTransaction>,
    extra: &TaikoExtra,
) -> Result<()> {
    let Some(anchor) = l2_fini.transactions.first().cloned() else {
        bail!("no anchor transaction found");
    };
    // - check anchor transaction
    precheck_anchor(l2_chain_spec, l2_fini, &anchor).with_context(|| "precheck anchor error")?;

    let mut txs: Vec<EthersTransaction> = vec![];
    // - tx list bytes must be less than MAX_TX_LIST_BYTES
    if extra.l2_tx_list.len() <= MAX_TX_LIST_BYTES {
        txs = rlp_decode_list(&extra.l2_tx_list).unwrap_or_else(|err| {
            tracing::error!("decode tx list error: {}", err);
            vec![]
        });
    } else {
        tracing::error!(
            "tx list bytes must be not more than MAX_TX_LIST_BYTES, got: {}",
            extra.l2_tx_list.len()
        );
    }
    // - tx list must be less than MAX_TX_LIST
    if txs.len() > MAX_TX_LIST {
        tracing::error!(
            "tx list must be not more than MAX_TX_LIST, got: {}",
            txs.len()
        );
        // reset to empty
        txs.clear();
    }
    // - patch anchor transaction into tx list instead of those from l2 node's
    // insert the anchor transaction into the tx list at the first position
    txs.insert(0, anchor);
    // reset transactions
    l2_fini.transactions = txs;
    Ok(())
}

#[derive(Debug)]
pub enum AnchorError {
    AnchorTypeMisMatch {
        expected: u8,
        got: u8,
    },
    AnchorFromMisMatch {
        expected: Address,
        got: Option<Address>,
    },
    AnchorToMisMatch {
        expected: Address,
        got: Option<Address>,
    },
    AnchorValueMisMatch {
        expected: EthersU256,
        got: EthersU256,
    },
    AnchorGasLimitMisMatch {
        expected: EthersU256,
        got: EthersU256,
    },
    AnchorFeeCapMisMatch {
        expected: Option<EthersU256>,
        got: Option<EthersU256>,
    },
    AnchorSignatureMismatch {
        msg: String,
    },
    Anyhow(anyhow::Error),
}

impl From<anyhow::Error> for AnchorError {
    fn from(e: anyhow::Error) -> Self {
        AnchorError::Anyhow(e)
    }
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self)
    }
}

impl Error for AnchorError {}

pub fn precheck_anchor(
    l2_chain_spec: &ChainSpec,
    l2_fini: &Block<EthersTransaction>,
    anchor: &EthersTransaction,
) -> Result<(), AnchorError> {
    let tx1559_type = U64::from(0x2);
    if anchor.transaction_type != Some(tx1559_type) {
        return Err(AnchorError::AnchorTypeMisMatch {
            expected: tx1559_type.as_u64() as u8,
            got: anchor.transaction_type.unwrap_or_default().as_u64() as u8,
        });
    }
    let tx: EthereumTransaction = anchor.clone().try_into()?;
    // verify transaction
    check_anchor_signature(&tx)?;
    // verify the transaction signature
    let from = from_ethers_h160(anchor.from);
    if from != *GOLDEN_TOUCH_ACCOUNT {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: *GOLDEN_TOUCH_ACCOUNT,
            got: Some(from),
        });
    }
    let Some(to) = anchor.to else {
        return Err(AnchorError::AnchorToMisMatch {
            expected: l2_chain_spec.l2_contract.unwrap(),
            got: None,
        });
    };
    let to = from_ethers_h160(to);
    if to != l2_chain_spec.l2_contract.unwrap() {
        return Err(AnchorError::AnchorFromMisMatch {
            expected: l2_chain_spec.l2_contract.unwrap(),
            got: Some(to),
        });
    }
    if anchor.value != EthersU256::zero() {
        return Err(AnchorError::AnchorValueMisMatch {
            expected: EthersU256::zero(),
            got: anchor.value,
        });
    }
    if anchor.gas != EthersU256::from(ANCHOR_GAS_LIMIT) {
        return Err(AnchorError::AnchorGasLimitMisMatch {
            expected: EthersU256::from(ANCHOR_GAS_LIMIT),
            got: anchor.gas,
        });
    }
    // anchor's gas price should be the same as the block's
    if anchor.max_fee_per_gas != l2_fini.base_fee_per_gas {
        return Err(AnchorError::AnchorFeeCapMisMatch {
            expected: l2_fini.base_fee_per_gas,
            got: anchor.max_fee_per_gas,
        });
    }
    Ok(())
}
