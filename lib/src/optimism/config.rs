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

use ruint::uint;
use serde::{Deserialize, Serialize};
use zeth_primitives::{address, Address};

use super::system_config::SystemConfig;

/// A Chain Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// The initial system config value
    pub system_config: SystemConfig,
    // The L1 attributes depositor address
    pub l1_attributes_depositor: Address,
    /// The L1 attributes contract
    pub l1_attributes_contract: Address,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
    /// The L1 system config contract
    pub system_config_contract: Address,
    /// The maximum byte size of all pending channels
    pub max_channel_size: u64,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub channel_timeout: u64,
    /// Number of L1 blocks in a sequence window
    pub seq_window_size: u64,
    /// Maximum timestamp drift
    pub max_seq_drift: u64,
    /// Network blocktime
    pub blocktime: u64,
}

impl ChainConfig {
    pub const fn optimism() -> Self {
        Self {
            system_config: SystemConfig {
                batch_sender: address!("6887246668a3b87f54deb3b94ba47a6f63f32985"),
                gas_limit: uint!(30_000_000_U256),
                l1_fee_overhead: uint!(188_U256),
                l1_fee_scalar: uint!(684000_U256),
                unsafe_block_signer: address!("AAAA45d9549EDA09E70937013520214382Ffc4A2"),
            },
            l1_attributes_depositor: address!("deaddeaddeaddeaddeaddeaddeaddeaddead0001"),
            l1_attributes_contract: address!("4200000000000000000000000000000000000015"),
            batch_inbox: address!("ff00000000000000000000000000000000000010"),
            deposit_contract: address!("bEb5Fc579115071764c7423A4f12eDde41f106Ed"),
            system_config_contract: address!("229047fed2591dbec1eF1118d64F7aF3dB9EB290"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            blocktime: 2,
        }
    }
}

pub const OPTIMISM_CHAIN_SPEC: ChainConfig = ChainConfig::optimism();
