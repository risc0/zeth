
use alloc::vec::Vec;

use alloy_primitives::{Address, B256};
use alloy_sol_types::{sol, SolValue};
use serde::{Deserialize, Serialize};
use zeth_primitives::keccak;

pub mod block_builder;
#[cfg(not(target_os = "zkvm"))]
pub mod execute;
pub mod prepare;
pub mod utils;

pub enum Layer {
    L1,
    L2,
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

    #[derive(Debug)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 signalRoot;
        bytes32 graffiti;
    }

    #[derive(Debug, Default, Clone, Deserialize, Serialize)]
    event BlockProposed(
        uint256 indexed blockId,
        address indexed prover,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );

    #[derive(Debug)]
    struct TierProof {
        uint16 tier;
        bytes data;
    }

    function proveBlock(uint64 blockId, bytes calldata input) {}
}

#[derive(Debug)]
pub enum EvidenceType {
    Sgx {
        new_pubkey: Address, // the evidence signature public key
    },
    PseZk,
}

#[derive(Debug)]
pub struct ProtocolInstance {
    pub transition: Transition,
    pub block_metadata: BlockMetadata,
    pub prover: Address,
}

impl ProtocolInstance {
    pub fn meta_hash(&self) -> B256 {
        keccak::keccak(self.block_metadata.abi_encode()).into()
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn hash(&self, evidence_type: EvidenceType) -> B256 {
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => keccak::keccak(
                (
                    self.transition.clone(),
                    new_pubkey,
                    self.prover,
                    self.meta_hash(),
                )
                    .abi_encode(),
            )
            .into(),
            EvidenceType::PseZk => todo!(),
        }
    }
}

pub fn deposits_hash(deposits: &[EthDeposit]) -> B256 {
    keccak::keccak(deposits.abi_encode()).into()
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolCall;

    use super::*;
    #[test]
    fn test_prove_block_call() {
        let input = "0x10d008bd000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000034057a97bd6f6930af5ca9e7caf48e663588755b690e9de0f82486416960135939559b91a6700c8af9442fe68f4339066d1d7858263c6be97ebcaca787ef70b1a7f8be37f1ab1fe1209f525f7cbced8a86ed49d1813849896c99835628f8eea703b302e31382e302d64657600000000000000000000000000000000000000000000569e75fc77c1a856f6daaf9e69d8a9566ca34aa47f9133711ce065a571af0cfd000000000000000000000000e1e210594771824dad216568b91c9cb4ceed361c000000000000000000000000000000000000000000000000000000000000299e0000000000000000000000000000000000000000000000000000000000e4e1c00000000000000000000000000000000000000000000000000000000065a63e6400000000000000000000000000000000000000000000000000000000000b6785000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000056220000000000000000000000000000000000000000000000000000000000000064000000000000000000000000000000000000000000000000000000000000000012d5f89f4195325e38f76ac324b08c34ab0c5c9ec430fc00dd967aa44b0bd05c11a7c619d13210437142d7adae4025ee65581228d0a8ed7a0df022634b2f1feadb23b17eaa3a5d3a7cfede2fa7d1653ac512117963c9fbe5f2df6a9dd555041ff20f4e661443b23d0c39ddbbb2725002cd2f7d5edb84d1c1eed9d8c71ddeba300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000028000000000000000000000000000000000000000000000000000000000000000c800000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000059000000000041035896fb7ccbed43b0fd70a82758535f3aa70e317bc173b815f18c416274d39cdd4918013cb12ccffc959700b8ae824b4a421d462c6fa19e28bdc64d6f753d978e0e76c33ce84aadfa19b68163c99dc62a631b00000000000000";

        let input_data = hex::decode(&input[2..]).unwrap();
        let proveBlockCall { blockId, input } =
            proveBlockCall::abi_decode(&input_data, false).unwrap();
        // println!("blockId: {}", blockId);
        let (meta, trans, proof) =
            <(BlockMetadata, Transition, TierProof)>::abi_decode_params(&input, false).unwrap();
        // println!("meta: {:?}", meta);
        let meta_hash: B256 = keccak::keccak(meta.abi_encode()).into();
        // println!("meta_hash: {:?}", meta_hash);
        // println!("trans: {:?}", trans);
        // println!("proof: {:?}", proof.tier);
        // println!("proof: {:?}", hex::encode(proof.data));
    }
}
