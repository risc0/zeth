use zeth_chainspec::MAINNET;
use zeth_core::EthEvmConfig;

pub fn main() {
    stateless_client::entry(EthEvmConfig::new(MAINNET.clone()));
}
