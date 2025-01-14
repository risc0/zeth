use alloy::signers::k256::ecdsa::VerifyingKey;
use alloy::{
    eips::eip1559::BaseFeeParams,
    network::{Ethereum, Network},
    primitives::{BlockNumber, B256, U256},
};
use reth_chainspec::{
    BaseFeeParamsKind, Chain, ChainHardforks, ChainSpec, EthereumHardfork, ForkCondition,
    NamedChain,
};
use std::{fmt::Display, sync::Arc};
use zeth_core::db::memory::MemoryDB;
use zeth_core::{
    driver::CoreDriver,
    stateless::{
        client::StatelessClient, data::StatelessClientData, execute::ExecutionStrategy,
        finalize::MemoryDbFinalizationStrategy, initialize::MemoryDbInitializationStrategy,
        validate::ValidationStrategy,
    },
};
use zeth_core_ethereum::{
    RethCoreDriver, RethExecutionStrategy, RethStatelessClient, RethValidationStrategy,
};
use zeth_preflight::{client::PreflightClient, driver::PreflightDriver, BlockBuilder};
use zeth_preflight_ethereum::{RethBlockBuilder, RethPreflightClient, RethPreflightDriver};

#[derive(Default, Copy, Clone, Debug)]
pub struct TestCoreDriver;

impl CoreDriver for TestCoreDriver {
    type ChainSpec = ChainSpec;
    type Block = reth_primitives::Block;
    type Header = reth_primitives::Header;
    type Receipt = reth_primitives::Receipt;
    type Transaction = reth_primitives::TransactionSigned;

    fn chain_spec(chain: &NamedChain) -> Option<Arc<Self::ChainSpec>> {
        let spec = ChainSpec {
            chain: Chain::from_named(chain.clone()),
            paris_block_and_final_difficulty: Some((0, U256::from(0))),
            hardforks: ChainHardforks::new(vec![
                (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
                (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
                (EthereumHardfork::Dao.boxed(), ForkCondition::Block(0)),
                (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
                (
                    EthereumHardfork::SpuriousDragon.boxed(),
                    ForkCondition::Block(0),
                ),
                (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
                (
                    EthereumHardfork::Constantinople.boxed(),
                    ForkCondition::Block(0),
                ),
                (
                    EthereumHardfork::Petersburg.boxed(),
                    ForkCondition::Block(0),
                ),
                (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
                (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
                (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
                (
                    EthereumHardfork::Paris.boxed(),
                    ForkCondition::TTD {
                        fork_block: None,
                        total_difficulty: U256::ZERO,
                    },
                ),
                (
                    EthereumHardfork::Shanghai.boxed(),
                    ForkCondition::Timestamp(0),
                ),
                (
                    EthereumHardfork::Cancun.boxed(),
                    ForkCondition::Timestamp(0),
                ),
            ]),
            base_fee_params: BaseFeeParamsKind::Constant(BaseFeeParams::ethereum()),
            ..Default::default()
        };
        Some(spec.into())
    }

    fn parent_hash(header: &Self::Header) -> B256 {
        RethCoreDriver::parent_hash(header)
    }

    fn header_hash(header: &Self::Header) -> B256 {
        RethCoreDriver::header_hash(header)
    }

    fn state_root(header: &Self::Header) -> B256 {
        RethCoreDriver::state_root(header)
    }

    fn block_number(header: &Self::Header) -> BlockNumber {
        RethCoreDriver::block_number(header)
    }

    fn block_header(block: &Self::Block) -> &Self::Header {
        RethCoreDriver::block_header(block)
    }

    fn block_to_header(block: Self::Block) -> Self::Header {
        RethCoreDriver::block_to_header(block)
    }

    fn accumulate_difficulty(total_difficulty: U256, header: &Self::Header) -> U256 {
        RethCoreDriver::accumulate_difficulty(total_difficulty, header)
    }

    fn final_difficulty(
        block: BlockNumber,
        total_difficulty: U256,
        chain_spec: &Self::ChainSpec,
    ) -> U256 {
        RethCoreDriver::final_difficulty(block, total_difficulty, chain_spec)
    }
}

impl BlockBuilder<'_, Ethereum, MemoryDB, TestCoreDriver, RethPreflightDriver>
    for RethBlockBuilder
{
    type PreflightClient = RethPreflightClient;
    type StatelessClient = RethStatelessClient;
}

impl PreflightClient<Ethereum, TestCoreDriver, RethPreflightDriver> for RethPreflightClient {
    type Validation = RethValidationStrategy;
    type Execution = RethExecutionStrategy;
}

impl StatelessClient<'_, TestCoreDriver, MemoryDB> for RethStatelessClient {
    type Initialization = MemoryDbInitializationStrategy;
    type Validation = RethValidationStrategy;
    type Execution = RethExecutionStrategy;
    type Finalization = MemoryDbFinalizationStrategy;
}

impl<Database> ValidationStrategy<TestCoreDriver, Database>
    for zeth_core_ethereum::RethValidationStrategy
where
    Database: 'static,
{
    fn validate_header(
        chain_spec: Arc<ChainSpec>,
        block: &mut reth_primitives::Block,
        parent_header: &mut alloy::consensus::Header,
        total_difficulty: &mut U256,
    ) -> anyhow::Result<()> {
        <RethValidationStrategy as ValidationStrategy<RethCoreDriver, Database>>::validate_header(
            chain_spec,
            block,
            parent_header,
            total_difficulty,
        )
    }
}

impl<Database: reth_revm::Database> ExecutionStrategy<TestCoreDriver, Database>
    for RethExecutionStrategy
where
    Database: 'static,
    <Database as reth_revm::Database>::Error:
        Into<reth_storage_errors::provider::ProviderError> + Display,
{
    fn execute_transactions(
        chain_spec: Arc<ChainSpec>,
        block: &mut reth_primitives::Block,
        signers: &[VerifyingKey],
        total_difficulty: &mut U256,
        db: &mut Option<Database>,
    ) -> anyhow::Result<reth_revm::db::BundleState> {
        <RethExecutionStrategy as ExecutionStrategy<RethCoreDriver, Database>>::execute_transactions(
            chain_spec,
            block,
            signers,
            total_difficulty,
            db,
        )
    }
}

impl PreflightDriver<TestCoreDriver, Ethereum> for RethPreflightDriver {
    fn total_difficulty(header: &<Ethereum as Network>::HeaderResponse) -> Option<U256> {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::total_difficulty(header)
    }

    fn count_transactions(block: &<Ethereum as Network>::BlockResponse) -> usize {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::count_transactions(
            block,
        )
    }

    fn derive_transaction(
        transaction: <Ethereum as Network>::TransactionResponse,
    ) -> <RethCoreDriver as CoreDriver>::Transaction {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_transaction(
            transaction,
        )
    }

    fn derive_header(header: <Ethereum as Network>::HeaderResponse) -> alloy::consensus::Header {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_header(header)
    }

    fn derive_block(
        block: <Ethereum as Network>::BlockResponse,
        ommers: Vec<<Ethereum as Network>::HeaderResponse>,
    ) -> reth_primitives::Block {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_block(
            block, ommers,
        )
    }

    fn derive_header_response(
        block: <Ethereum as Network>::BlockResponse,
    ) -> <Ethereum as Network>::HeaderResponse {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_header_response(
            block,
        )
    }

    fn header_response(
        block: &<Ethereum as Network>::BlockResponse,
    ) -> &<Ethereum as Network>::HeaderResponse {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::header_response(block)
    }

    fn uncles(block: &<Ethereum as Network>::BlockResponse) -> &Vec<B256> {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::uncles(block)
    }

    fn derive_receipt(
        receipt: <Ethereum as Network>::ReceiptResponse,
    ) -> <RethCoreDriver as CoreDriver>::Receipt {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_receipt(receipt)
    }

    fn derive_data(
        data: StatelessClientData<
            <Ethereum as Network>::BlockResponse,
            <Ethereum as Network>::HeaderResponse,
        >,
        ommers: Vec<Vec<<Ethereum as Network>::HeaderResponse>>,
    ) -> StatelessClientData<reth_primitives::Block, alloy::consensus::Header> {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::derive_data(
            data, ommers,
        )
    }

    fn recover_signers(block: &<TestCoreDriver as CoreDriver>::Block) -> Vec<VerifyingKey> {
        <RethPreflightDriver as PreflightDriver<RethCoreDriver, Ethereum>>::recover_signers(block)
    }
}
