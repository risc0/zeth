// Copyright 2025 RISC Zero, Inc.
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

use alloy_consensus::Header;
use alloy_eips::{
    BlobScheduleBlobParams,
    eip1559::BaseFeeParams,
    eip2124::{ForkFilter, ForkId, Head},
};
use alloy_evm::eth::spec::EthExecutorSpec;
use alloy_genesis::Genesis;
use alloy_hardforks::{EthereumHardfork, EthereumHardforks, ForkCondition, Hardfork};
use alloy_primitives::{Address, B256, U256, address};
use reth_chainspec::{Chain, DepositContract, EthChainSpec, Hardforks, NamedChain};
use std::{
    any::Any,
    collections::BTreeMap,
    fmt::{self, Debug, Display},
    sync::{Arc, LazyLock},
};

const MAINNET_DEPOSIT_CONTRACT_ADDRESS: Address =
    address!("0x00000000219ab540356cbb839cbe05303d7705fa");
const SEPOLIA_DEPOSIT_CONTRACT_ADDRESS: Address =
    address!("0x7f02c3e3c98b133055b8b348b2ac625669ed295d");
const HOLESKY_DEPOSIT_CONTRACT_ADDRESS: Address =
    address!("0x4242424242424242424242424242424242424242");

pub static MAINNET: LazyLock<Arc<ChainSpec>> = LazyLock::new(|| {
    let spec = ChainSpec {
        chain: NamedChain::Mainnet.into(),
        forks: EthereumHardfork::mainnet().into(),
        deposit_contract_address: Some(MAINNET_DEPOSIT_CONTRACT_ADDRESS),
        blob_params: BlobScheduleBlobParams::mainnet(),
    };
    spec.into()
});

pub static SEPOLIA: LazyLock<Arc<ChainSpec>> = LazyLock::new(|| {
    let spec = ChainSpec {
        chain: NamedChain::Sepolia.into(),
        forks: EthereumHardfork::sepolia().into(),
        deposit_contract_address: Some(SEPOLIA_DEPOSIT_CONTRACT_ADDRESS),
        blob_params: BlobScheduleBlobParams::mainnet(),
    };
    spec.into()
});

pub static HOLESKY: LazyLock<Arc<ChainSpec>> = LazyLock::new(|| {
    let spec = ChainSpec {
        chain: NamedChain::Holesky.into(),
        forks: EthereumHardfork::holesky().into(),
        deposit_contract_address: Some(HOLESKY_DEPOSIT_CONTRACT_ADDRESS),
        blob_params: BlobScheduleBlobParams::mainnet(),
    };
    spec.into()
});

#[derive(Clone, Debug)]
pub struct ChainSpec {
    chain: Chain,
    forks: BTreeMap<EthereumHardfork, ForkCondition>,
    deposit_contract_address: Option<Address>,
    blob_params: BlobScheduleBlobParams,
}

impl Display for ChainSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.chain)
    }
}

impl EthereumHardforks for ChainSpec {
    fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
        self.forks.get(&fork).cloned().unwrap_or_default()
    }
}

impl EthExecutorSpec for ChainSpec {
    fn deposit_contract_address(&self) -> Option<Address> {
        self.deposit_contract_address
    }
}

impl Hardforks for ChainSpec {
    fn fork<H: Hardfork>(&self, fork: H) -> ForkCondition {
        if let Some(eth_fork) = (&fork as &dyn Any).downcast_ref::<EthereumHardfork>() {
            self.ethereum_fork_activation(*eth_fork)
        } else {
            ForkCondition::Never
        }
    }

    fn forks_iter(&self) -> impl Iterator<Item = (&dyn Hardfork, ForkCondition)> {
        self.forks.iter().map(|(eth_fork, condition)| (eth_fork as &dyn Hardfork, *condition))
    }

    fn fork_id(&self, _: &Head) -> ForkId {
        unimplemented!()
    }

    fn latest_fork_id(&self) -> ForkId {
        unimplemented!()
    }

    fn fork_filter(&self, _: Head) -> ForkFilter {
        unimplemented!()
    }
}

impl EthChainSpec for ChainSpec {
    type Header = Header;

    fn chain(&self) -> Chain {
        self.chain
    }

    fn base_fee_params_at_block(&self, _: u64) -> BaseFeeParams {
        unimplemented!()
    }

    fn base_fee_params_at_timestamp(&self, _: u64) -> BaseFeeParams {
        unimplemented!()
    }

    fn blob_params_at_timestamp(&self, timestamp: u64) -> Option<alloy_eips::eip7840::BlobParams> {
        if let Some(blob_param) = self.blob_params.active_scheduled_params_at_timestamp(timestamp) {
            Some(*blob_param)
        } else if self.is_osaka_active_at_timestamp(timestamp) {
            Some(self.blob_params.osaka)
        } else if self.is_prague_active_at_timestamp(timestamp) {
            Some(self.blob_params.prague)
        } else if self.is_cancun_active_at_timestamp(timestamp) {
            Some(self.blob_params.cancun)
        } else {
            None
        }
    }

    fn deposit_contract(&self) -> Option<&DepositContract> {
        unimplemented!()
    }

    fn genesis_hash(&self) -> B256 {
        unimplemented!()
    }

    fn prune_delete_limit(&self) -> usize {
        unimplemented!()
    }

    fn display_hardforks(&self) -> Box<dyn Display> {
        unimplemented!()
    }

    fn genesis_header(&self) -> &Self::Header {
        unimplemented!()
    }

    fn genesis(&self) -> &Genesis {
        unimplemented!()
    }

    fn bootnodes(&self) -> Option<Vec<reth_network_peers::node_record::NodeRecord>> {
        unimplemented!()
    }

    fn final_paris_total_difficulty(&self) -> Option<U256> {
        if let ForkCondition::TTD { total_difficulty, .. } =
            self.ethereum_fork_activation(EthereumHardfork::Paris)
        {
            Some(total_difficulty)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_eq(spec: &ChainSpec, reth_spec: &reth_chainspec::ChainSpec) {
        assert_eq!(spec.chain, reth_spec.chain);
        assert_eq!(spec.blob_params, reth_spec.blob_params);
        assert_eq!(
            spec.forks.values().cloned().collect::<Vec<_>>(),
            reth_spec.forks_iter().map(|(_, f)| f).collect::<Vec<_>>(),
        );
        assert_eq!(spec.deposit_contract_address, reth_spec.deposit_contract.map(|c| c.address),);
    }

    #[test]
    fn mainnet() {
        assert_eq(&MAINNET, &reth_chainspec::MAINNET);
    }

    #[test]
    fn sepolia() {
        assert_eq(&SEPOLIA, &reth_chainspec::SEPOLIA);
    }

    #[test]
    fn holesky() {
        assert_eq(&HOLESKY, &reth_chainspec::HOLESKY);
    }
}
