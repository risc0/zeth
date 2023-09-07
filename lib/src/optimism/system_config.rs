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

use anyhow::{self, bail, ensure, Context, Ok};
use serde::{Deserialize, Serialize};
use zeth_primitives::{b256, Address, Bloom, BloomInput, B256, U256};

use super::{config::ChainConfig, epoch::BlockInput};

/// Signature of the deposit transaction event, i.e.
/// keccak-256 hash of "ConfigUpdate(uint256,uint8,bytes)"
const CONFIG_UPDATE_SIGNATURE: B256 =
    b256!("1d2b0bda21d56b8bd12d4f94ebacffdfb35f5e226f84b461103bb8beab6353be");
/// Version of the deposit transaction event.
const CONFIG_UPDATE_VERSION: B256 = B256::ZERO;

/// Optimism system config contract values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    /// Batch sender address
    pub batch_sender: Address,
    /// L2 gas limit
    pub gas_limit: U256,
    /// Fee overhead
    pub l1_fee_overhead: U256,
    /// Fee scalar
    pub l1_fee_scalar: U256,
    /// Sequencer's signer for unsafe blocks
    pub unsafe_block_signer: Address,
}

impl SystemConfig {
    /// Updates the system config based on the given input. Returns whether the config was
    /// updated.
    pub fn update(&mut self, config: &ChainConfig, input: &BlockInput) -> anyhow::Result<bool> {
        let mut updated = false;

        // if the bloom filter does not contain the corresponding topics, we have the guarantee
        // that there are no config updates in the block
        if !can_contain(
            &config.system_config_contract,
            &input.block_header.logs_bloom,
        ) {
            return Ok(updated);
        }

        let receipts = input.receipts.as_ref().context("receipts missing")?;
        for receipt in receipts {
            let receipt = &receipt.payload;

            // skip failed transactions
            if !receipt.success {
                continue;
            }

            for log in &receipt.logs {
                // the log event contract address must match the system config contract
                // the first log event topic must match the ConfigUpdate signature
                if log.address == config.system_config_contract
                    && log.topics[0] == CONFIG_UPDATE_SIGNATURE
                {
                    updated = true;

                    // the second topic determines the version
                    ensure!(log.topics[1] == CONFIG_UPDATE_VERSION, "invalid version");

                    // the third topic determines the type of update
                    let update_type: u64 = U256::from_be_bytes(log.topics[2].0)
                        .try_into()
                        .context("invalid update type")?;

                    // TODO: use proper ABI decoding of the data
                    match update_type {
                        // type 0: batcherHash overwrite, as bytes32 payload
                        0 => {
                            let addr_bytes = log
                                .data
                                .get(76..96)
                                .context("invalid batch sender address")?;

                            self.batch_sender = Address::from_slice(addr_bytes);
                        }
                        // type 1: overhead and scalar overwrite, as two packed uint256 entries
                        1 => {
                            let fee_overhead = log.data.get(64..96).context("invalid data")?;
                            let fee_scalar = log.data.get(96..128).context("invalid data")?;

                            self.l1_fee_overhead = U256::try_from_be_slice(fee_overhead)
                                .context("invalid overhead")?;
                            self.l1_fee_scalar =
                                U256::try_from_be_slice(fee_scalar).context("invalid scalar")?;
                        }
                        // type 2: gasLimit overwrite, as uint64 payload
                        2 => {
                            let gas_limit = log.data.get(64..96).context("invalid data")?;

                            self.gas_limit =
                                U256::try_from_be_slice(gas_limit).context("invalid gas limit")?;
                        }
                        // type 3: unsafeBlockSigner overwrite, as address payload
                        3 => {
                            let addr_bytes = log
                                .data
                                .get(76..96)
                                .context("invalid unsafe block signer address")?;

                            self.unsafe_block_signer = Address::from_slice(addr_bytes);
                        }
                        _ => {
                            bail!("invalid update type");
                        }
                    }
                }
            }
        }

        Ok(updated)
    }
}

/// Returns whether the given Bloom filter can contain a config update log.
pub fn can_contain(address: &Address, bloom: &Bloom) -> bool {
    let input = BloomInput::Raw(address.as_slice());
    if !bloom.contains_input(input) {
        return false;
    }
    let input = BloomInput::Raw(CONFIG_UPDATE_SIGNATURE.as_slice());
    if !bloom.contains_input(input) {
        return false;
    }
    true
}
