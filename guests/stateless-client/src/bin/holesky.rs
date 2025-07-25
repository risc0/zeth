use zeth_chainspec::HOLESKY;
use zeth_core::EthEvmConfig;

pub fn main() {
    stateless_client::entry(EthEvmConfig::new(HOLESKY.clone()));
}
