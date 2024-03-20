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
use core::fmt::Debug;

use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_rpc_types::Withdrawal as AlloyWithdrawal;
use alloy_sol_types::{sol, SolCall};
use anyhow::{anyhow, Result};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use zeth_primitives::{mpt::MptNode, Address, Bytes, FixedBytes, B256, U256};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuestInput {
    /// Block hash - for reference!
    pub block_hash: B256,
    /// Previous block header
    pub parent_header: AlloyConsensusHeader,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: Address,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: u64,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: u64,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of transactions for execution
    // pub transactions: Vec<Transaction<E>>,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<AlloyWithdrawal>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<AlloyConsensusHeader>,
    /// Base fee per gas
    pub base_fee_per_gas: u64,
    /// Taiko specific data
    pub taiko: TaikoGuestInput,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaikoGuestInput {
    pub chain_spec_name: String,
    pub l1_header: AlloyConsensusHeader,
    pub tx_list: Vec<u8>,
    pub anchor_tx: String,
    pub block_proposed: BlockProposed,
    pub prover_data: TaikoProverData,
    pub tx_blob_hash: Option<B256>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct TaikoProverData {
    pub prover: Address,
    pub graffiti: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuestOutput {
    Success((AlloyConsensusHeader, FixedBytes<32>)),
    Failure,
}

sol! {
    function anchor(
        bytes32 l1Hash,
        //bytes32 l1StateRoot,
        //uint64 l1BlockId,
        bytes32 l1SignalRoot,
        uint64 l1Height,
        uint32 parentGasUsed
    )
        external
    {}
}

#[inline]
pub fn decode_anchor(bytes: &[u8]) -> Result<anchorCall> {
    anchorCall::abi_decode(bytes, true).map_err(|e| anyhow!(e))
    // .context("Invalid anchor call")
}

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
        uint96 amount;
        uint64 id;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadata {
        bytes32 l1Hash;
        bytes32 difficulty;
        bytes32 blobHash; //or txListHash (if Blob not yet supported)
        bytes32 extraData;
        bytes32 depositsHash;
        address coinbase; // L2 coinbase
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp;
        uint64 l1Height;
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash;
        address sender;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockParams {
        address assignedProver;
        address coinbase;
        bytes32 extraData;
        bytes32 parentMetaHash;
        HookCall[] hookCalls;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct HookCall {
        address hook;
        bytes data;
    }

    #[derive(Debug)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        //bytes32 stateRoot;
        bytes32 graffiti;
    }

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    event BlockProposed(
        uint256 indexed blockId,
        address indexed assignedProver,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );

    #[derive(Debug)]
    struct TierProof {
        uint16 tier;
        bytes data;
    }

    #[derive(Debug)]
    function proposeBlock(
        bytes calldata params,
        bytes calldata txList
    )
    {}

    function proveBlock(uint64 blockId, bytes calldata input) {}
}

pub mod protocol_testnet {
    use alloy_sol_types::sol;
    use serde::{Deserialize, Serialize};

    sol! {
        #[derive(Debug, Default, Deserialize, Serialize)]
        struct EthDeposit {
            address recipient;
            uint96 amount;
            uint64 id;
        }

        #[derive(Debug, Default, Deserialize, Serialize)]
        struct BlockMetadata {
            bytes32 l1Hash; // slot 1
            bytes32 difficulty; // slot 2
            bytes32 blobHash; //or txListHash (if Blob not yet supported), // slot 3
            bytes32 extraData; // slot 4
            bytes32 depositsHash; // slot 5
            address coinbase; // L2 coinbase, // slot 6
            uint64 id;
            uint32 gasLimit;
            uint64 timestamp; // slot 7
            uint64 l1Height;
            uint24 txListByteOffset;
            uint24 txListByteSize;
            uint16 minTier;
            bool blobUsed;
            bytes32 parentMetaHash; // slot 8
        }

        #[derive(Debug, Default, Deserialize, Serialize)]
        struct BlockParams {
            address assignedProver;
            address coinbase;
            bytes32 extraData;
            bytes32 blobHash;
            uint24 txListByteOffset;
            uint24 txListByteSize;
            bool cacheBlobForReuse;
            bytes32 parentMetaHash;
            HookCall[] hookCalls;
        }

        #[derive(Debug, Default, Deserialize, Serialize)]
        struct HookCall {
            address hook;
            bytes data;
        }

        #[derive(Debug)]
        struct Transition {
            bytes32 parentHash;
            bytes32 blockHash;
            bytes32 signalRoot;
            //bytes32 stateRoot;
            bytes32 graffiti;
        }

        #[derive(Debug, Default, Clone, Deserialize, Serialize)]
        event BlockProposed(
            uint256 indexed blockId,
            address indexed assignedProver,
            uint96 livenessBond,
            BlockMetadata meta,
            EthDeposit[] depositsProcessed
        );

        #[derive(Debug)]
        struct TierProof {
            uint16 tier;
            bytes data;
        }

        #[derive(Debug)]
        function proposeBlock(
            bytes calldata params,
            bytes calldata txList
        )
        {}

        function proveBlock(uint64 blockId, bytes calldata input) {}
    }
}

impl From<protocol_testnet::EthDeposit> for EthDeposit {
    fn from(item: protocol_testnet::EthDeposit) -> Self {
        EthDeposit {
            recipient: item.recipient,
            amount: item.amount,
            id: item.id,
        }
    }
}

impl From<protocol_testnet::BlockProposed> for BlockProposed {
    fn from(item: protocol_testnet::BlockProposed) -> Self {
        BlockProposed {
            blockId: item.blockId,
            assignedProver: item.assignedProver,
            livenessBond: item.livenessBond,
            meta: BlockMetadata {
                l1Hash: item.meta.l1Hash,
                difficulty: item.meta.difficulty,
                blobHash: item.meta.blobHash,
                extraData: item.meta.extraData,
                depositsHash: item.meta.depositsHash,
                coinbase: item.meta.coinbase,
                id: item.meta.id,
                gasLimit: item.meta.gasLimit,
                timestamp: item.meta.timestamp,
                l1Height: item.meta.l1Height,
                minTier: item.meta.minTier,
                blobUsed: item.meta.blobUsed,
                parentMetaHash: item.meta.parentMetaHash,
                ..Default::default()
            },
            depositsProcessed: item
                .depositsProcessed
                .iter()
                .map(|v| v.clone().into())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = GuestInput {
            block_hash: Default::default(),
            parent_header: Default::default(),
            beneficiary: Default::default(),
            gas_limit: Default::default(),
            timestamp: Default::default(),
            extra_data: Default::default(),
            mix_hash: Default::default(),
            // transactions: vec![],
            withdrawals: vec![],
            parent_state_trie: Default::default(),
            parent_storage: Default::default(),
            contracts: vec![],
            ancestor_headers: vec![],
            base_fee_per_gas: Default::default(),
            taiko: Default::default(),
        };
        let _: GuestInput = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
