use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_rlp::{Decodable, Encodable};
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use super::signature::TxSignature;
use crate::transactions::{
    ethereum::{EthereumTxEssence, TransactionKind},
    SignedDecodable, TxEssence,
};

pub const LINEA_DEPOSITEDP_TX_TYPE: u8 = 0x7E; // Check!

#[derive(
    Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, RlpEncodable, RlpDecodable,
)]
pub struct TxEssenceLineaDeposited {
    pub soruce_hash: B256,
    pub from: Address,
    pub to: TransactionKind,
    pub mint: U256,
    pub value: U256,
    pub gas_limit: U256,
    pub is_system_tx: bool,
    pub data: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineaTxEssence {
    Ethereum(EthereumTxEssence),
    LineaDeposited(TxEssenceLineaDeposited),
}

impl Encodable for LineaTxEssence {
    #[inline]
    fn encode(&self, out: &mut dyn BufMut) {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.encode(out),
            LineaTxEssence::LineaDeposited(linea) => linea.encode(out),
        }
    }

    #[inline]
    fn length(&self) -> usize {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.length(),
            LineaTxEssence::LineaDeposited(linea) => linea.length(),
        }
    }
}

impl SignedDecodable<TxSignature> for LineaTxEssence {
    fn decode_signed(buf: &mut &[u8]) -> alloy_rlp::Result<(Self, TxSignature)> {
        match buf.first().copied() {
            Some(0x7e) => {
                buf.advance(1);
                Ok((
                    LineaTxEssence::LineaDeposited(TxEssenceLineaDeposited::decode(buf)?),
                    TxSignature::default(),
                ))
            }
            Some(_) => {
                EthereumTxEssence::decode_signed(buf).map(|(e, s)| (LineaTxEssence::Ethereum(e), s))
            }
            None => Err(alloy_rlp::Error::InputTooShort),
        }
    }
}

impl TxEssence for LineaTxEssence {
    fn tx_type(&self) -> u8 {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.tx_type(),
            LineaTxEssence::LineaDeposited(_) => LINEA_DEPOSITEDP_TX_TYPE,
        }
    }

    fn gas_limit(&self) -> U256 {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.gas_limit(),
            LineaTxEssence::LineaDeposited(linea) => linea.gas_limit,
        }
    }

    fn to(&self) -> Option<Address> {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.to(),
            LineaTxEssence::LineaDeposited(linea) => linea.to.into(),
        }
    }

    fn recover_from(&self, signature: &TxSignature) -> anyhow::Result<Address> {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.recover_from(signature),
            LineaTxEssence::LineaDeposited(linea) => Ok(linea.from),
        }
    }

    fn payload_length(&self) -> usize {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.payload_length(),
            LineaTxEssence::LineaDeposited(linea) => linea._alloy_rlp_payload_length(),
        }
    }

    fn data(&self) -> &Bytes {
        match self {
            LineaTxEssence::Ethereum(eth) => eth.data(),
            LineaTxEssence::LineaDeposited(linea) => &linea.data,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use serde_json::json;

    use super::*;
    use crate::{
        transactions::{LineaTransaction, Transaction},
        RlpBytes,
    };
    // TODO: Test Linea Deposited

    #[test]
    fn ethereum() {
        // Tx: 0x8a81acdd462bf1fa28f70bbfb3ffdc16faeb129150b188a412a531e73c5f48d6
        let tx = json!({
            "Ethereum": {
                "Legacy": {
                  "chain_id": 59144,
                  "nonce": 693,
                  "gas_price": "0x49410d40",
                  "gas_limit": "0x12fc0",
                  "to": { "Call": "0x1bf74c010e6320bab11e2e5a532b5aC15e0b8aa6" },
                  "value": "0x0",
                  "data": "0x095ea7b3000000000000000000000000de1e598b81620773454588b85d6b5d4eec32573e0000000000000000000000000000000000000000000000000e9e6db806658000",
                }
            }
        });

        let essence: LineaTxEssence = serde_json::from_value(tx).unwrap();

        let signature: TxSignature = serde_json::from_value(json!({
            "v": 118323,
            "r": "0x9139574d640491c6be24e22b6de271e84f7af94fd67728d5e55767f902a35019",
            "s": "0x647839c257718f5dac2af7b1f8a6e5e584cda3519f61a7f5ee229581bb0728c2"
        }))
        .unwrap();

        let transaction = LineaTransaction { essence, signature };

        // verify the RLP roundtrip
        let decoded = Transaction::decode_bytes(alloy_rlp::encode(&transaction)).unwrap();
        assert_eq!(transaction, decoded);

        let _: LineaTransaction =
            bincode::deserialize(&bincode::serialize(&transaction).unwrap()).unwrap();

        let encoded = alloy_rlp::encode(&transaction);
        assert_eq!(encoded.len(), transaction.length());

        assert_eq!(
            transaction.hash(),
            b256!("8a81acdd462bf1fa28f70bbfb3ffdc16faeb129150b188a412a531e73c5f48d6")
        );
        let recovered = transaction.recover_from().unwrap();
        assert_eq!(
            recovered,
            address!("5dDeF75a8C992d0fdcb55cCC0195E70698113573")
        );
    }
}
