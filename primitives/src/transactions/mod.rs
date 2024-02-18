// Copyright 2024 RISC Zero, Inc.
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

use std::fmt::Debug;

pub use alloy_consensus::{TxEip1559, TxEip2930, TxEip4844, TxLegacy};
pub use alloy_network::SignableTransaction;
use alloy_network::{
    eip2718::{Decodable2718, Eip2718Error, Encodable2718},
    Signed, Transaction as _,
};
use alloy_primitives::B256;
use alloy_rlp::{Decodable, Encodable};
use serde::{Deserialize, Deserializer, Serialize};

use self::optimism::{TxOptimismDeposit, OPTIMISM_DEPOSITED_TX_TYPE};
use crate::RlpBytes;

// pub mod ethereum;
pub mod optimism;

/// Represents a minimal EVM transaction.
pub trait EvmTransaction: Encodable + Decodable {
    /// Recover `from`.
    fn from(&self) -> Result<alloy_primitives::Address, alloy_primitives::SignatureError>;
    /// Get `to`.
    fn to(&self) -> Option<alloy_primitives::Address>;
    /// Get `gas_limit`.
    fn gas_limit(&self) -> u64;
    /// Get `data`.
    fn input(&self) -> &[u8];
    /// Returns the cached transaction hash.
    fn hash(&self) -> B256;
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum TxType {
    /// Legacy transaction type.
    Legacy = 0,
    /// EIP-2930 transaction type.
    Eip2930 = 1,
    /// EIP-1559 transaction type.
    Eip1559 = 2,
    /// EIP-4844 transaction type.
    Eip4844 = 3,
    /// Optimism deposited transaction type.
    OptimismDeposit = OPTIMISM_DEPOSITED_TX_TYPE,
}

impl TryFrom<u8> for TxType {
    type Error = Eip2718Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            // SAFETY: repr(u8) with explicit discriminant
            0..=3 => Ok(unsafe { std::mem::transmute(value) }),
            OPTIMISM_DEPOSITED_TX_TYPE => Ok(TxType::OptimismDeposit),
            _ => Err(Eip2718Error::UnexpectedType(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxEnvelope {
    /// An untagged [`TxLegacy`].
    Legacy(Signed<TxLegacy>),
    /// A [`TxEip2930`].
    Eip2930(Signed<TxEip2930>),
    /// A [`TxEip1559`].
    Eip1559(Signed<TxEip1559>),
    /// A [`TxEip4844`].
    Eip4844(Signed<TxEip4844>),
    /// An [`TxOptimismDeposited`].
    OptimismDeposit(TxOptimismDeposit),
}

impl TxEnvelope {
    /// Return the [`TxType`] of the inner txn.
    pub const fn tx_type(&self) -> TxType {
        match self {
            Self::Legacy(_) => TxType::Legacy,
            Self::Eip2930(_) => TxType::Eip2930,
            Self::Eip1559(_) => TxType::Eip1559,
            Self::Eip4844(_) => TxType::Eip4844,
            Self::OptimismDeposit(_) => TxType::OptimismDeposit,
        }
    }

    fn inner_encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match self {
            Self::Legacy(t) => t.encode(out),
            Self::Eip2930(t) => t.encode(out),
            Self::Eip1559(t) => t.encode(out),
            Self::Eip4844(t) => t.encode(out),
            Self::OptimismDeposit(t) => t.encode(out),
        }
    }

    fn inner_length(&self) -> usize {
        match self {
            Self::Legacy(t) => t.length(),
            Self::Eip2930(t) => t.length(),
            Self::Eip1559(t) => t.length(),
            Self::Eip4844(t) => t.length(),
            Self::OptimismDeposit(t) => t.length(),
        }
    }
}

impl EvmTransaction for TxEnvelope {
    fn from(&self) -> Result<alloy_primitives::Address, alloy_primitives::SignatureError> {
        match self {
            Self::Legacy(tx) => tx.recover_signer(),
            Self::Eip2930(tx) => tx.recover_signer(),
            Self::Eip1559(tx) => tx.recover_signer(),
            Self::Eip4844(tx) => tx.recover_signer(),
            Self::OptimismDeposit(tx) => Ok(tx.from()),
        }
    }

    fn to(&self) -> Option<alloy_primitives::Address> {
        match self {
            Self::Legacy(tx) => tx.to().to(),
            Self::Eip2930(tx) => tx.to().to(),
            Self::Eip1559(tx) => tx.to().to(),
            Self::Eip4844(tx) => tx.to().to(),
            Self::OptimismDeposit(tx) => tx.to().to(),
        }
    }

    fn gas_limit(&self) -> u64 {
        match self {
            Self::Legacy(tx) => tx.gas_limit(),
            Self::Eip2930(tx) => tx.gas_limit(),
            Self::Eip1559(tx) => tx.gas_limit(),
            Self::Eip4844(tx) => tx.gas_limit(),
            Self::OptimismDeposit(tx) => tx.gas_limit(),
        }
    }

    fn input(&self) -> &[u8] {
        match self {
            Self::Legacy(tx) => tx.input(),
            Self::Eip2930(tx) => tx.input(),
            Self::Eip1559(tx) => tx.input(),
            Self::Eip4844(tx) => tx.input(),
            Self::OptimismDeposit(tx) => tx.input(),
        }
    }

    fn hash(&self) -> B256 {
        match self {
            Self::Legacy(tx) => *tx.hash(),
            Self::Eip2930(tx) => *tx.hash(),
            Self::Eip1559(tx) => *tx.hash(),
            Self::Eip4844(tx) => *tx.hash(),
            // TODO: cache the hash for `OptimismDeposited`
            Self::OptimismDeposit(tx) => tx.hash_slow(),
        }
    }
}

impl Serialize for TxEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bytes = alloy_rlp::encode(self);
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TxEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<TxEnvelope, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes = <Vec<u8>>::deserialize(deserializer)?;
        Self::decode_bytes(bytes).map_err(serde::de::Error::custom)
    }
}

impl Encodable for TxEnvelope {
    #[inline]
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.encode_2718(out);
    }
    #[inline]
    fn length(&self) -> usize {
        self.encode_2718_len()
    }
}

impl Decodable for TxEnvelope {
    #[inline]
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        match Self::decode_2718(buf) {
            Ok(tx) => Ok(tx),
            Err(Eip2718Error::RlpError(e)) => Err(e),
            Err(_) => Err(alloy_rlp::Error::Custom("Unexpected type")),
        }
    }
}

impl Encodable2718 for TxEnvelope {
    fn type_flag(&self) -> Option<u8> {
        match self {
            TxEnvelope::Legacy(_) => None,
            TxEnvelope::Eip2930(_) => Some(TxType::Eip2930 as u8),
            TxEnvelope::Eip1559(_) => Some(TxType::Eip1559 as u8),
            TxEnvelope::Eip4844(_) => Some(TxType::Eip4844 as u8),
            TxEnvelope::OptimismDeposit(_) => Some(OPTIMISM_DEPOSITED_TX_TYPE),
        }
    }

    fn encode_2718_len(&self) -> usize {
        match self {
            Self::Legacy(tx) => tx.length(),
            _ => 1 + self.inner_length(),
        }
    }

    fn encode_2718(&self, out: &mut dyn bytes::BufMut) {
        match self {
            Self::Legacy(tx) => tx.encode(out),
            _ => {
                out.put_u8(self.tx_type() as u8);
                self.inner_encode(out);
            }
        }
    }
}

impl Decodable2718 for TxEnvelope {
    fn typed_decode(ty: u8, buf: &mut &[u8]) -> Result<Self, Eip2718Error> {
        match ty.try_into()? {
            TxType::Legacy => unreachable!(),
            TxType::Eip2930 => Ok(Self::Eip2930(Decodable::decode(buf)?)),
            TxType::Eip1559 => Ok(Self::Eip1559(Decodable::decode(buf)?)),
            TxType::Eip4844 => Ok(Self::Eip4844(Decodable::decode(buf)?)),
            TxType::OptimismDeposit => Ok(Self::OptimismDeposit(Decodable::decode(buf)?)),
        }
    }

    fn fallback_decode(buf: &mut &[u8]) -> Result<Self, Eip2718Error> {
        Ok(TxEnvelope::Legacy(Decodable::decode(buf)?))
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};
    use hex_literal::hex;

    use super::*;
    use crate::RlpBytes;

    #[test]
    fn legacy() {
        // Tx: 0x5c504ed432cb51138bcf09aa5e8a410dd4a1e204ef84bfed1be16dfba1b22060
        let raw_tx = hex!("f86780862d79883d2000825208945df9b87991262f6ba471f09758cde1c0fc1de734827a69801ca088ff6cf0fefd94db46111149ae4bfc179e9b94721fffd821d38d16464b3f71d0a045e0aff800961cfce805daef7016b9b675c137a6a41a548f7b60a3484c06a33a");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let decoded = TxEnvelope::decode_bytes(alloy_rlp::encode(&tx)).unwrap();
        assert_eq!(tx, decoded);
        // test the bincode roundtrip
        let _: TxEnvelope = bincode::deserialize(&bincode::serialize(&tx).unwrap()).unwrap();

        assert_eq!(
            tx.hash(),
            b256!("5c504ed432cb51138bcf09aa5e8a410dd4a1e204ef84bfed1be16dfba1b22060")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("A1E4380A3B1f749673E270229993eE55F35663b4")
        );
    }

    #[test]
    fn eip155() {
        // Tx: 0x4540eb9c46b1654c26353ac3c65e56451f711926982ce1b02f15c50e7459caf7
        let raw_tx = hex!("f870830834a08503c49bfa0483019a2894f0ee707731d1be239f9f482e1b2ea5384c0c426f8806df842eaa9fb8008026a0cadd790a37b78e5613c8cf44dc3002e3d7f06a5325d045963c708efe3f9fdf7aa01f63adb9a2d5e020c6aa0ff64695e25d7d9a780ed8471abe716d2dc0bf7d4259");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let decoded = TxEnvelope::decode_bytes(alloy_rlp::encode(&tx)).unwrap();
        assert_eq!(tx, decoded);

        assert_eq!(
            tx.hash(),
            b256!("4540eb9c46b1654c26353ac3c65e56451f711926982ce1b02f15c50e7459caf7")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("974CaA59e49682CdA0AD2bbe82983419A2ECC400")
        );
    }

    #[test]
    fn eip2930() {
        // Tx: 0xbe4ef1a2244e99b1ef518aec10763b61360be22e3b649dcdf804103719b1faef
        let raw_tx = hex!("01f903050183016e97850f46a5a9d88302167094c11ce44147c9f6149fbe54adb0588523c38718d784010d1471b841050000000002b8809aef26206090eafd7d5688615d48197d1c5ce09be6c30a33be4c861dee44d13f6dd33c2e8c5cad7e2725f88a8f0000000002d67ca5eb0e5fb6f90253f87a94d6e64961ba13ba42858ad8a74ed9a9b051a4957df863a00000000000000000000000000000000000000000000000000000000000000008a00b4b38935f88a7bddbe6be76893de2a04640a55799d6160729a82349aff1ffaea0c59ee2ee2ba599569b2b1f06989dadbec5ee157c8facfe64f36a3e33c2b9d1bff87a94c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2f863a07635825e4f8dfeb20367f8742c8aac958a66caa001d982b3a864dcc84167be80a042555691810bdf8f236c31de88d2cc9407a8ff86cd230ba3b7029254168df92aa029ece5a5f4f3e7751868475502ab752b5f5fa09010960779bf7204deb72f5ddef89b944c861dee44d13f6dd33c2e8c5cad7e2725f88a8ff884a0000000000000000000000000000000000000000000000000000000000000000ca00000000000000000000000000000000000000000000000000000000000000008a00000000000000000000000000000000000000000000000000000000000000006a00000000000000000000000000000000000000000000000000000000000000007f8bc9490eafd7d5688615d48197d1c5ce09be6c30a33bef8a5a00000000000000000000000000000000000000000000000000000000000000001a09c04773acff4c5c42718bd0120c72761f458e43068a3961eb935577d1ed4effba00000000000000000000000000000000000000000000000000000000000000008a00000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000000000000000000000000000000000000000000000401a0f86aa2dfde99b0d6a41741e96cfcdee0c6271febd63be4056911db19ae347e66a0601deefbc4835cb15aa1af84af6436fc692dea3428d53e7ff3d34a314cefe7fc");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let decoded = TxEnvelope::decode_bytes(alloy_rlp::encode(&tx)).unwrap();
        assert_eq!(tx, decoded);

        assert_eq!(
            tx.hash(),
            b256!("be4ef1a2244e99b1ef518aec10763b61360be22e3b649dcdf804103719b1faef")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("79b7a69d90c82E014Bf0315e164208119B510FA0")
        );
    }

    #[test]
    fn eip1559() {
        // Tx: 0x2bcdc03343ca9c050f8dfd3c87f32db718c762ae889f56762d8d8bdb7c5d69ff
        let raw_tx = hex!("02f8730120843b9aca0085089d5f3200825b0494a9d1e08c7793af67e9d92fe308d5697fb81d3e438801dd1f234f68cde280c080a02bdf47562da5f2a09f09cce70aed35ec9ac62f5377512b6a04cc427e0fda1f4da028f9311b515a5f17aa3ad5ea8bafaecfb0958801f01ca11fd593097b5087121b");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let decoded = TxEnvelope::decode_bytes(alloy_rlp::encode(&tx)).unwrap();
        assert_eq!(tx, decoded);

        assert_eq!(
            tx.hash(),
            b256!("2bcdc03343ca9c050f8dfd3c87f32db718c762ae889f56762d8d8bdb7c5d69ff")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("4b9f4114D50e7907BFF87728A060Ce8d53Bf4CF7")
        );
    }

    #[test]
    fn eip4844() {
        // Tx: 0x25f463005fb95770cfe9fffd857dc6b20878b32fa179cd235552027898da0c8e
        let raw_tx = hex!("03f89883aa36a7820348843b9aca008502568f7a7682520894ff000000000000000000000000000000111554218080c08508e82ced46e1a00153f8e9343d99868798a9b41d4fe990c5384db8b0fd68a9189ecf1f9a6deec780a0c9fb7d6c3ade31a242dfaf2004cec17ddb82c24f7ff7516a519b27bb7f8f7808a01674813859d3b1e282b7789391896cf9945349cf05d8550ab81f3a2cdc99dc12");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let decoded = TxEnvelope::decode_bytes(alloy_rlp::encode(&tx)).unwrap();
        assert_eq!(tx, decoded);

        assert_eq!(
            tx.hash(),
            b256!("25f463005fb95770cfe9fffd857dc6b20878b32fa179cd235552027898da0c8e")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("19CC7073150D9f5888f09E0e9016d2a39667df14")
        );
    }

    #[test]
    fn optimism_deposited() {
        // Tx: 0x2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3
        let raw_tx = hex!("7ef90209a020b925f36904e1e62099920d902925817c4357e9f674b8b14d133631961390109436bde71c97b33cc4729cf772ae268934f7ab70b294420000000000000000000000000000000000000788030d98d59a96000088030d98d59a96000083077d2e80b901a4d764ad0b000100000000000000000000000000000000000000000000000000000000af8600000000000000000000000099c9fc46f92e8a1c0dec1b1747d010903e884be10000000000000000000000004200000000000000000000000000000000000010000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000030d4000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000000a41635f5fd000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000ab12275f2d91f87b301a4f01c9af4e83b3f45baa000000000000000000000000000000000000000000000000030d98d59a9600000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        let tx = TxEnvelope::decode_2718(&mut raw_tx.as_slice()).unwrap();
        assert_eq!(tx.encode_2718_len(), raw_tx.len());
        println!("{:#?}", tx);

        // verify the RLP roundtrip
        let encoded = alloy_rlp::encode(&tx);
        assert_eq!(encoded.len(), tx.length());
        let decoded = TxEnvelope::decode_bytes(encoded).unwrap();
        assert_eq!(tx, decoded);

        assert_eq!(
            tx.hash(),
            b256!("2bf9119d4faa19593ca1b3cda4b4ac03c0ced487454a50fbdcd09aebe21210e3")
        );
        assert_eq!(
            tx.from().unwrap(),
            address!("36bde71c97b33cc4729cf772ae268934f7ab70b2")
        );
    }
}
