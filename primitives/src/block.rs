// Copyright 2023 RISC Zero, Inc.
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

use alloy_primitives::{b256, BlockHash, BlockNumber, Bloom, Bytes, B160, B256, B64, U256};
use alloy_rlp_derive::RlpEncodable;
use serde::{Deserialize, Serialize};

use crate::{keccak::keccak, trie::EMPTY_ROOT};

/// Keccak-256 hash of the RLP of an empty list, keccak256("\xc0").
pub const EMPTY_LIST_HASH: B256 =
    b256!("1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RlpEncodable)]
#[rlp(trailing)]
pub struct Header {
    /// Hash of the parent block's header.
    pub parent_hash: BlockHash,
    /// Unused 256-bit hash, always [EMPTY_LIST_HASH].
    pub ommers_hash: B256,
    /// Address to which the priority fees of each transaction is transferred.
    pub beneficiary: B160,
    /// Hash of the root node of the state trie, after all transactions are executed.
    pub state_root: B256,
    /// Hash of the root node of the trie populated with each transaction in the block.
    pub transactions_root: B256,
    /// Hash of the root node of the trie populated with the receipts of each transaction.
    pub receipts_root: B256,
    /// Bloom filter composed from indexable information contained in each log entry.
    pub logs_bloom: Bloom,
    /// Unused value, always `0`.
    pub difficulty: U256,
    /// Number of ancestor blocks in the chain.
    pub number: BlockNumber,
    /// Value equal to the current limit of gas expenditure per block.
    pub gas_limit: U256,
    /// Value equal to the total gas used in transactions in this block.
    pub gas_used: U256,
    /// Value corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: U256,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// Unused 64-bit hash, always zero.
    pub nonce: B64,
    /// Base fee payed by all transactions in the block.
    pub base_fee_per_gas: U256,
    /// Hash of the root node of the trie populated with each withdrawal in the block.
    /// Only present after the Shanghai update.
    #[serde(default)]
    pub withdrawals_root: Option<B256>,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            parent_hash: B256::ZERO,
            ommers_hash: EMPTY_LIST_HASH,
            beneficiary: B160::ZERO,
            state_root: EMPTY_ROOT,
            transactions_root: EMPTY_ROOT,
            receipts_root: EMPTY_ROOT,
            logs_bloom: Bloom::default(),
            difficulty: U256::ZERO,
            number: 0,
            gas_limit: U256::ZERO,
            gas_used: U256::ZERO,
            timestamp: U256::ZERO,
            extra_data: Bytes::new(),
            mix_hash: B256::ZERO,
            nonce: B64::ZERO,
            base_fee_per_gas: U256::ZERO,
            withdrawals_root: None,
        }
    }
}

impl Header {
    /// Calculates the block hash.
    pub fn hash(&self) -> BlockHash {
        keccak(alloy_rlp::encode(self)).into()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn paris() {
        // first block after the Paris network upgrade
        let value = json!({
            "parent_hash":"0x55b11b918355b1ef9c5db810302ebad0bf2544255b530cdce90674d5887bb286",
            "ommers_hash": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "beneficiary": "0xeee27662c2b8eba3cd936a23f039f3189633e4c8",
            "state_root": "0x40c07091e16263270f3579385090fea02dd5f061ba6750228fcc082ff762fda7",
            "transactions_root": "0x1ea1746468686159ce730c1cc49a886721244e5d1fa9a06d6d4196b6f013c82c",
            "receipts_root": "0x928073fb98ce316265ea35d95ab7e2e1206cecd85242eb841dbbcc4f568fca4b",
            "logs_bloom": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "difficulty": "0x0",
            "number": 15537394,
            "gas_limit": "0x1c9c380",
            "gas_used": "0x1c9811e",
            "timestamp": "0x6322c973",
            "extra_data": "0x",
            "mix_hash": "0xa86c2e601b6c44eb4848f7d23d9df3113fbcac42041c49cbed5000cb4f118777",
            "nonce": "0x0000000000000000",
            "base_fee_per_gas": "0xb5d68e0a3"
        });
        let header: Header = serde_json::from_value(value).unwrap();

        // verify that bincode serialization works
        let _: Header = bincode::deserialize(&bincode::serialize(&header).unwrap()).unwrap();

        assert_eq!(
            "0x56a9bb0302da44b8c0b3df540781424684c3af04d0b7a38d72842b762076a664",
            header.hash().to_string()
        )
    }

    #[test]
    fn shanghai() {
        // first block after the Shanghai network upgrade
        let value = json!({
            "parent_hash": "0xc2558f8143d5f5acb8382b8cb2b8e2f1a10c8bdfeededad850eaca048ed85d8f",
            "ommers_hash": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "beneficiary": "0x388c818ca8b9251b393131c08a736a67ccb19297",
            "state_root": "0x7fd42f5027bc18315b3781e65f19e4c8828fd5c5fce33410f0fb4fea0b65541f",
            "transactions_root": "0x6f235d618461c08943aa5c23cc751310d6177ab8a9b9a7b66ffa637d988680e6",
            "receipts_root": "0xe0ac34bafdd757bcca2dea27a3fc5870dd0836998877e29361c1fc55e19416ec",
            "logs_bloom": "0xb06769bc11f4d7a51a3bc4bed59367b75c32d1bd79e5970e73732ac0eed0251af0e2abc8811fc1b4c5d45a4a4eb5c5af9e73cc9a8be6ace72faadc03536d6b69fcdf80116fd89f7efbdbf38ff957e8f6ae83ccac60cf4b7c8b1c9487bebfa8ed6e42297e17172d5b678dd3f283b22f49bbf4a0565eb93d9d797b2f9a0adaff9813af53d6fffa71d5a6fb056ab73ca87659dc97c19f99839c6c3138e527161b4dfee8b1f64d42f927abc745f3ff168e8e9510e2e079f4868ba8ff94faf37c9a7947a43c1b4c931dfbef88edeb2d7ede5ceaebc85095cfbbd206646def0138683b687fa63fdf22898260d616bc714d698bc5748c7a5bff0a4a32dd797596a794a0",
            "difficulty": "0x0",
            "number": 17034870,
            "gas_limit": "0x1c9c380",
            "gas_used": "0x1c9bfe2",
            "timestamp": "0x6437306f",
            "extra_data": "0xd883010b05846765746888676f312e32302e32856c696e7578",
            "mix_hash": "0x812ed704cc408c435c7baa6e86296c1ac654a139ae8c4a26d6460742b951d4f9",
            "nonce": "0x0000000000000000",
            "base_fee_per_gas": "0x42fbae6d5",
            "withdrawals_root": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
        });
        let header: Header = serde_json::from_value(value).unwrap();

        // verify that bincode serialization works
        let _: Header = bincode::deserialize(&bincode::serialize(&header).unwrap()).unwrap();

        assert_eq!(
            "0xe22c56f211f03baadcc91e4eb9a24344e6848c5df4473988f893b58223f5216c",
            header.hash().to_string()
        )
    }
}
