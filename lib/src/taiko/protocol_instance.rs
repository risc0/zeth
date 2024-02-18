use alloc::vec::Vec;
use alloc::format;
use alloy_primitives::{Address, TxHash, B256};
use alloy_sol_types::SolValue;
use anyhow::{anyhow, ensure, Result};
use revm::primitives::SpecId;
use zeth_primitives::{
    block::Header,
    keccak::keccak,
    transactions::EthereumTransaction,
};

use super::{consts::ANCHOR_GAS_LIMIT, BlockMetadata, EthDeposit, TaikoSystemInfo, Transition};
use crate::consts::TKO_MAINNET_CHAIN_SPEC;

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
            EvidenceType::Pse => todo!(),
            EvidenceType::Powdr => todo!(),
            EvidenceType::Succinct => keccak(
                (
                    self.transition.clone(),
                    // no pubkey since we don't need TEE to sign
                    self.prover,
                    self.meta_hash(),
                )
                    .abi_encode(),
            ).into(),
        }
    }
}

#[derive(Debug)]
pub enum EvidenceType {
    Sgx {
        new_pubkey: Address, // the evidence signature public key
    },
    Pse,
    Powdr,
    Succinct,
}

// TODO(cecilia): rewrite
pub fn assemble_protocol_instance(
    sys: &TaikoSystemInfo,
    header: &Header,
) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak(sys.l2_tx_list.as_slice()));
    let block_hash: zeth_primitives::U256 = tx_list_hash.into();

    let deposits = sys
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect::<Vec<_>>();
    let deposits_hash: B256 = keccak(deposits.abi_encode()).into();

    let extra_data: B256 = bytes_to_bytes32(&header.extra_data).into();

    let prevrandao = if TKO_MAINNET_CHAIN_SPEC.spec_id(header.number) >= SpecId::SHANGHAI {
        sys.l1_next_block.mix_hash.into()
    } else {
        sys.l1_next_block.difficulty
    };
    let difficulty = block_hash
        ^ (prevrandao
            * zeth_primitives::U256::from(header.number)
            * zeth_primitives::U256::from(sys.l1_next_block.number));

    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let mut pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: sys.l2_signal_root,
            graffiti: sys.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: sys.l1_hash,
            difficulty: difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data,
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: sys.l1_height,
            txListByteOffset: 0u32,
            txListByteSize: sys.l2_tx_list.len() as u32,
            minTier: sys.block_proposed.meta.minTier,
            blobUsed: sys.l2_tx_list.is_empty(),
            parentMetaHash: sys.block_proposed.meta.parentMetaHash,
        },
        prover: sys.prover,
    };
    verify(sys, header, &mut pi)?;
    Ok(pi)
}

pub fn verify(sys: &TaikoSystemInfo, header: &Header, pi: &mut ProtocolInstance) -> Result<()> {
    // check the block metadata
    ensure!(
        pi.block_metadata.abi_encode() == sys.block_proposed.meta.abi_encode(),
        format!("block hash mismatch, expected: {:?}, got: {:?}",  sys.block_proposed.meta, pi.block_metadata)
    );
    ensure!(
        header.hash() == sys.l2_block.hash(),
        format!("block hash mismatch, expected: {:?}, got: {:?}",  header.hash(), sys.l2_block.hash())
    );

    Ok(())
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
