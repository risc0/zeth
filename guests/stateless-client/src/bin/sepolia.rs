use zeth_chainspec::SEPOLIA;
use zeth_core::EthEvmConfig;

pub fn main() {
    stateless_client::entry(EthEvmConfig::new(SEPOLIA.clone()));
}
