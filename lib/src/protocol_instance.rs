use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_primitives::{Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{ensure, Result};
use zeth_primitives::keccak::keccak;

use super::taiko_utils::ANCHOR_GAS_LIMIT;
use crate::{
    input::{BlockMetadata, EthDeposit, GuestInput, Transition},
    taiko_utils::HeaderHasher,
};

#[derive(Debug)]
pub struct ProtocolInstance {
    pub transition: Transition,
    pub block_metadata: BlockMetadata,
    pub prover: Address,
}

impl ProtocolInstance {
    pub fn meta_hash(&self) -> B256 {
        keccak(self.block_metadata.abi_encode()).into()
    }

    // keccak256(abi.encode(tran, newInstance, prover, metaHash))
    pub fn instance_hash(&self, evidence_type: EvidenceType) -> B256 {
        match evidence_type {
            EvidenceType::Sgx { new_pubkey } => keccak(
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
            EvidenceType::Powdr => todo!(),
            EvidenceType::Succinct => keccak(
                (
                    self.transition.clone(),
                    // no pubkey since we don't need TEE to sign
                    self.prover,
                    self.meta_hash(),
                )
                    .abi_encode(),
            )
            .into(),
            EvidenceType::Risc0 => {
                keccak((self.transition.clone(), self.prover, self.meta_hash()).abi_encode()).into()
            }
            EvidenceType::Native => {
                keccak((self.transition.clone(), self.prover, self.meta_hash()).abi_encode()).into()
            }
        }
    }
}

#[derive(Debug)]
pub enum EvidenceType {
    Sgx {
        new_pubkey: Address, // the evidence signature public key
    },
    PseZk,
    Powdr,
    Succinct,
    Risc0,
    Native,
}

// TODO(cecilia): rewrite
pub fn assemble_protocol_instance(
    input: &GuestInput,
    header: &AlloyConsensusHeader,
) -> Result<ProtocolInstance> {
    let blob_used = input.taiko.block_proposed.meta.blobUsed;
    let tx_list_hash = if blob_used {
        input.taiko.tx_blob_hash.unwrap()
    } else {
        TxHash::from(keccak(input.taiko.tx_list.as_slice()))
    };

    let deposits = input
        .withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect::<Vec<_>>();

    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: input.taiko.l1_header.state_root,
            graffiti: input.taiko.prover_data.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: input.taiko.l1_header.hash(),
            difficulty: input.taiko.block_proposed.meta.difficulty,
            blobHash: tx_list_hash,
            extraData: bytes_to_bytes32(&header.extra_data).into(),
            depositsHash: keccak(deposits.abi_encode()).into(),
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: input.taiko.l1_header.number.try_into().unwrap(),
            minTier: input.taiko.block_proposed.meta.minTier,
            blobUsed: blob_used,
            parentMetaHash: input.taiko.block_proposed.meta.parentMetaHash,
            sender: input.taiko.block_proposed.meta.sender,
        },
        prover: input.taiko.prover_data.prover,
    };

    // Sanity check
    ensure!(
        pi.block_metadata.abi_encode() == input.taiko.block_proposed.meta.abi_encode(),
        format!(
            "block hash mismatch, expected: {:?}, got: {:?}",
            input.taiko.block_proposed.meta, pi.block_metadata
        )
    );

    Ok(pi)
}

fn bytes_to_bytes32(input: &[u8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let len = core::cmp::min(input.len(), 32);
    bytes[..len].copy_from_slice(&input[..len]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_to_bytes32_test() {
        let input = "";
        let byte = bytes_to_bytes32(input.as_bytes());
        assert_eq!(
            byte,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
    }
}
