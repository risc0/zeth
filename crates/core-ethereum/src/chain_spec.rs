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

use alloy_genesis::Genesis;
use once_cell::sync::Lazy;
use reth_chainspec::{
    BaseFeeParams, BaseFeeParamsKind, Chain, ChainSpec, DepositContract, EthereumHardfork,
    DEV_HARDFORKS, MAINNET_PRUNE_DELETE_LIMIT,
};
use reth_revm::primitives::{address, b256, bytes, U256};
use std::sync::Arc;

/// The Ethereum mainnet spec
pub static MAINNET: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    ChainSpec {
        chain: Chain::mainnet(),
        genesis: {
            let mut genesis = Genesis::default()
                .with_nonce(0x42)
                .with_extra_data(bytes!(
                    "11bbe8db4e347b4e8c937c1c8370e4b5ed33adb3db69cbdb7a38e1e50b1b82fa"
                ))
                .with_gas_limit(0x1388)
                .with_difficulty(U256::from(0x400000000u128));
            genesis.config.dao_fork_support = true;
            genesis
        },
        genesis_header: Default::default(),
        // <https://etherscan.io/block/15537394>
        paris_block_and_final_difficulty: Some((
            15537394,
            U256::from(58_750_003_716_598_352_816_469u128),
        )),
        hardforks: EthereumHardfork::mainnet().into(),
        // https://etherscan.io/tx/0xe75fb554e433e03763a1560646ee22dcb74e5274b34c5ad644e7c0f619a7e1d0
        deposit_contract: Some(DepositContract::new(
            address!("00000000219ab540356cbb839cbe05303d7705fa"),
            11052984,
            b256!("649bbc62d0e31342afea4e5cd82d4049e7e1ee912fc0889aa790803be39038c5"),
        )),
        base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
        prune_delete_limit: MAINNET_PRUNE_DELETE_LIMIT,
        blob_params: Default::default(),
    }
    .into()
});

/// The Sepolia spec
pub static SEPOLIA: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    ChainSpec {
        chain: Chain::sepolia(),
        genesis: {
            let mut genesis = Genesis::default()
                .with_timestamp(0x6159af19)
                .with_extra_data(bytes!(
                    "5365706f6c69612c20417468656e732c204174746963612c2047726565636521"
                ))
                .with_gas_limit(0x1c9c380)
                .with_difficulty(U256::from(0x20000u128));
            genesis.config.dao_fork_support = true;
            genesis
        },
        genesis_header: Default::default(),
        // <https://sepolia.etherscan.io/block/1450409>
        paris_block_and_final_difficulty: Some((1450409, U256::from(17_000_018_015_853_232u128))),
        hardforks: EthereumHardfork::sepolia().into(),
        // https://sepolia.etherscan.io/tx/0x025ecbf81a2f1220da6285d1701dc89fb5a956b62562ee922e1a9efd73eb4b14
        deposit_contract: Some(DepositContract::new(
            address!("7f02c3e3c98b133055b8b348b2ac625669ed295d"),
            1273020,
            b256!("649bbc62d0e31342afea4e5cd82d4049e7e1ee912fc0889aa790803be39038c5"),
        )),
        base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
        prune_delete_limit: 10000,
        blob_params: Default::default(),
    }
    .into()
});

/// The Holesky spec
pub static HOLESKY: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    ChainSpec {
        chain: Chain::holesky(),
        genesis: {
            let mut genesis = Genesis::default()
                .with_nonce(0x1234)
                .with_timestamp(1695902100)
                .with_extra_data(bytes!("017D7840"))
                .with_difficulty(U256::from(0x01u128));
            genesis.config.dao_fork_support = true;
            genesis
        },
        genesis_header: Default::default(),
        paris_block_and_final_difficulty: Some((0, U256::from(1))),
        hardforks: EthereumHardfork::holesky().into(),
        deposit_contract: Some(DepositContract::new(
            address!("4242424242424242424242424242424242424242"),
            0,
            b256!("649bbc62d0e31342afea4e5cd82d4049e7e1ee912fc0889aa790803be39038c5"),
        )),
        base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
        prune_delete_limit: 10000,
        blob_params: Default::default(),
    }
    .into()
});

/// Dev testnet specification
///
/// Includes 20 prefunded accounts with `10_000` ETH each derived from mnemonic "test test test test
/// test test test test test test test junk".
pub static DEV: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    ChainSpec {
        chain: Chain::dev(),
        genesis: Genesis::default(),
        paris_block_and_final_difficulty: Some((0, U256::from(0))),
        hardforks: DEV_HARDFORKS.clone(),
        base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
        deposit_contract: None, // TODO: do we even have?
        ..Default::default()
    }
    .into()
});
