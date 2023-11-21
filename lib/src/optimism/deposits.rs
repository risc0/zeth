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

use alloy_sol_types::{sol_data, SolType};
use anyhow::{ensure, Context};
use zeth_primitives::{
    fixed_bytes, keccak256,
    receipt::Log,
    transactions::{
        ethereum::{EthereumTxEssence, TransactionKind},
        optimism::{OptimismTxEssence, TxEssenceOptimismDeposited},
        Transaction,
    },
    Address, Bloom, BloomInput, B256, U160, U256,
};

use super::{config::ChainConfig, epoch::BlockInput};

/// Signature of the deposit transaction event, i.e.
/// keccak-256 hash of "TransactionDeposited(address,address,uint256,bytes)"
const TRANSACTION_DEPOSITED_SIGNATURE: B256 =
    fixed_bytes!("b3813568d9991fc951961fcb4c784893574240a28925604d09fc577c55bb7c32");
/// Version of the deposit transaction event.
const TRANSACTION_DEPOSITED_VERSION: B256 = B256::ZERO;

/// Extracts deposits from the given block.
pub fn extract_transactions(
    config: &ChainConfig,
    input: &BlockInput<EthereumTxEssence>,
) -> anyhow::Result<Vec<Transaction<OptimismTxEssence>>> {
    let block_hash = input.block_header.hash();

    // if the bloom filter does not contain the corresponding topics, we have the guarantee
    // that there are no deposits in the block
    if !can_contain(&config.deposit_contract, &input.block_header.logs_bloom) {
        return Ok(vec![]);
    }

    let receipts = input.receipts.as_ref().context("receipts missing")?;

    let mut deposits = Vec::new();

    let mut log_index = 0_usize;
    for receipt in receipts {
        let receipt = &receipt.payload;

        // skip failed transactions
        if !receipt.success {
            log_index += receipt.logs.len();
            continue;
        }
        // we could skip the transaction if the Bloom filter does not contain the deposit log, but
        // since hashing is quite expensive on the guest, it seems faster to always check the
        // logs

        // parse all the logs for deposit transactions
        for log in &receipt.logs {
            if log.address == config.deposit_contract
                && log.topics[0] == TRANSACTION_DEPOSITED_SIGNATURE
            {
                deposits.push(
                    to_deposit_transaction(block_hash, log_index, log)
                        .context("invalid deposit")?,
                );
            }

            log_index += 1;
        }
    }

    Ok(deposits)
}

/// Returns whether the given Bloom filter can contain a deposit log.
pub fn can_contain(address: &Address, bloom: &Bloom) -> bool {
    return true; // TODO: remove me!

    let input = BloomInput::Raw(address.as_slice());
    if !bloom.contains_input(input) {
        return false;
    }
    let input = BloomInput::Raw(TRANSACTION_DEPOSITED_SIGNATURE.as_slice());
    if !bloom.contains_input(input) {
        return false;
    }
    true
}

/// Converts a deposit log into a transaction.
fn to_deposit_transaction(
    block_hash: B256,
    log_index: usize,
    log: &Log,
) -> anyhow::Result<Transaction<OptimismTxEssence>> {
    let from = U160::try_from_be_slice(log.topics[1].as_slice())
        .context("invalid from")?
        .into();
    let to = U160::try_from_be_slice(log.topics[2].as_slice())
        .context("invalid to")?
        .into();

    // TODO: it is not 100% defined, what happens if the version is not 0
    // it is assumed that this is an error and must not be ignored
    ensure!(
        log.topics[3] == TRANSACTION_DEPOSITED_VERSION,
        "invalid version"
    );

    // the log data is just an ABI encoded `bytes` type representing the opaque_data
    let opaque_data: Vec<u8> =
        sol_data::Bytes::abi_decode(&log.data, true).context("invalid data")?;

    ensure!(opaque_data.len() >= 73, "invalid opaque_data");
    let mint = U256::try_from_be_slice(&opaque_data[0..32]).context("invalid mint")?;
    let value = U256::try_from_be_slice(&opaque_data[32..64]).context("invalid value")?;
    let gas_limit = U256::try_from_be_slice(&opaque_data[64..72]).context("invalid gas_limit")?;
    let is_creation = opaque_data[72] != 0;
    let data = opaque_data[73..].to_vec();

    // compute the source hash
    let h = keccak256([block_hash.0, U256::from(log_index).to_be_bytes()].concat());
    let source_hash = keccak256([U256::from(0).to_be_bytes(), h.0].concat());

    // construct the transaction
    let essence = OptimismTxEssence::OptimismDeposited(TxEssenceOptimismDeposited {
        source_hash,
        from,
        to: if is_creation {
            TransactionKind::Create
        } else {
            TransactionKind::Call(to)
        },
        mint,
        value,
        gas_limit,
        is_system_tx: false,
        data: data.into(),
    });

    Ok(Transaction {
        essence,
        signature: Default::default(),
    })
}
