use anyhow::Result;
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h256, from_ethers_u256},
    keccak,
    taiko::{
        deposits_hash, string_to_bytes32, BlockMetadata, EthDeposit, ProtocolInstance, Transition,
        ANCHOR_GAS_LIMIT,
    },
    TxHash, U256,
};

use crate::taiko::host::TaikoExtra;

pub fn assemble_protocol_instance(extra: &TaikoExtra, header: &Header) -> Result<ProtocolInstance> {
    let tx_list_hash = TxHash::from(keccak::keccak(extra.l2_tx_list.as_slice()));
    let deposits: Vec<EthDeposit> = extra
        .l2_withdrawals
        .iter()
        .map(|w| EthDeposit {
            recipient: w.address,
            amount: w.amount as u128,
            id: w.index,
        })
        .collect();
    let deposits_hash = deposits_hash(&deposits);
    let extra_data = string_to_bytes32(&header.extra_data);
    //   meta.difficulty = meta.blobHash ^ bytes32(block.prevrandao * b.numBlocks *
    // block.number);
    let block_hash = tx_list_hash;
    let block_hash_h256: U256 = block_hash.into();
    let prevrando = if cfg!(feature = "pos") {
        from_ethers_h256(extra.l1_next_block.mix_hash.unwrap_or_default()).into()
    } else {
        from_ethers_u256(extra.l1_next_block.difficulty)
    };
    let difficulty = block_hash_h256
        ^ (prevrando
            * U256::from(header.number)
            * U256::from(extra.l1_next_block.number.unwrap_or_default().as_u64()));
    let gas_limit: u64 = header.gas_limit.try_into().unwrap();
    let mut pi = ProtocolInstance {
        transition: Transition {
            parentHash: header.parent_hash,
            blockHash: header.hash(),
            signalRoot: extra.l2_signal_root,
            graffiti: extra.graffiti,
        },
        block_metadata: BlockMetadata {
            l1Hash: extra.l1_hash,
            difficulty: difficulty.into(),
            blobHash: tx_list_hash,
            extraData: extra_data.into(),
            depositsHash: deposits_hash,
            coinbase: header.beneficiary,
            id: header.number,
            gasLimit: (gas_limit - ANCHOR_GAS_LIMIT) as u32,
            timestamp: header.timestamp.try_into().unwrap(),
            l1Height: extra.l1_height,
            txListByteOffset: 0u32,
            txListByteSize: extra.l2_tx_list.len() as u32,
            minTier: extra.block_proposed.meta.minTier,
            blobUsed: extra.l2_tx_list.is_empty(),
            parentMetaHash: extra.block_proposed.meta.parentMetaHash,
        },
        prover: extra.prover,
    };
    #[cfg(not(target_os = "zkvm"))]
    {
        crate::taiko::verify::verify(header, &mut pi, extra)?;
    }
    Ok(pi)
}
