use crate::db::PreflightDB;
use crate::derive::{RPCDerivableBlock, RPCDerivableData, RPCDerivableHeader};
use crate::provider::db::ProviderDB;
use crate::provider::{new_provider, BlockQuery, UncleQuery};
use crate::trie::extend_proof_tries;
use alloy::rpc::types::{Block, Header};
use anyhow::Context;
use hashbrown::HashSet;
use log::{debug, info};
use reth_chainspec::ChainSpec;
use reth_evm_ethereum::execute::EthBatchExecutor;
use reth_evm_ethereum::EthEvmConfig;
use reth_revm::db::{BundleState, OriginalValuesKnown};
use reth_revm::primitives::Bytecode;
use std::iter::zip;
use std::mem::replace;
use std::path::PathBuf;
use std::sync::Arc;
use zeth_core::mpt::MptNode;
use zeth_core::rescue::Wrapper;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core::stateless::driver::{RethDriver, SCEDriver};
use zeth_core::stateless::engine::StatelessClientEngine;
use zeth_core::stateless::execute::{
    DbExecutionInput, RethExecStrategy, TransactionExecutionStrategy,
};
use zeth_core::stateless::post_exec::{PostExecutionValidationStrategy, RethPostExecStrategy};
use zeth_core::stateless::pre_exec::{
    ConsensusPreExecValidationInput, PreExecutionValidationStrategy, RethPreExecStrategy,
};

pub trait PreflightClient<B: RPCDerivableBlock, H: RPCDerivableHeader, R: SCEDriver<B, H>> {
    type PreExecValidation: for<'a> PreExecutionValidationStrategy<
        B,
        H,
        PreflightDB,
        Input<'a> = ConsensusPreExecValidationInput<'a, B, H>,
    >;
    type TransactionExecution: for<'a, 'b> TransactionExecutionStrategy<
        B,
        H,
        Wrapper<PreflightDB>,
        Input<'a> = DbExecutionInput<'a, B, Wrapper<PreflightDB>>,
        Output<'b> = EthBatchExecutor<EthEvmConfig, Wrapper<PreflightDB>>,
    >;
    type PostExecValidation: for<'a, 'b> PostExecutionValidationStrategy<
        B,
        H,
        Wrapper<PreflightDB>,
        Input<'a> = <Self::TransactionExecution as TransactionExecutionStrategy<
            B,
            H,
            Wrapper<PreflightDB>,
        >>::Output<'a>,
        Output<'b> = BundleState,
    >;

    fn preflight(
        chain_spec: Arc<ChainSpec>,
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
        block_count: u64,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        let provider = new_provider(cache_dir.clone(), block_no, rpc_url.clone())?;
        let mut provider_mut = provider.borrow_mut();
        // Fetch the parent block
        let parent_block = provider_mut.get_full_block(&BlockQuery {
            block_no: block_no - 1,
        })?;
        debug!(
            "Initial block: {:?} ({:?})",
            parent_block.header.number, parent_block.header.hash
        );
        let parent_header = parent_block.header;

        // Fetch the blocks and their uncles
        info!("Grabbing blocks and their uncles ...");
        let mut blocks = Vec::new();
        let mut ommers = Vec::new();
        for block_no in block_no..block_no + block_count {
            let block = provider_mut.get_full_block(&BlockQuery { block_no })?;
            let uncle_headers: Vec<_> = block
                .uncles
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    provider_mut
                        .get_uncle_block(&UncleQuery {
                            block_no,
                            uncle_index: idx as u64,
                        })
                        .expect("Failed to retrieve uncle block")
                        .header
                })
                .collect();
            // Print Debug info
            debug!(
                "Block number: {:?} ({:?})",
                block.header.number, block.header.hash,
            );
            debug!("Transaction count: {:?}", block.transactions.len());
            debug!("Uncle count: {:?}", block.uncles.len());
            // Collect data
            blocks.push(block);
            ommers.push(uncle_headers);
            // Prepare for next iteration
            provider_mut.save()?;
            provider_mut.advance()?;
        }
        ommers.reverse();

        // Create the provider DB with a fresh provider to reset block_no
        let provider_db = ProviderDB::new(
            new_provider(cache_dir, block_no, rpc_url)?,
            parent_header.number,
        );
        let preflight_db = PreflightDB::from(provider_db);

        // Create the input data
        let total_difficulty = parent_header
            .total_difficulty
            .expect("Missing total difficulty");
        let data = StatelessClientData {
            blocks: blocks.into_iter().rev().collect(),
            state_trie: Default::default(),
            storage_tries: Default::default(),
            contracts: vec![],
            parent_header,
            ancestor_headers: vec![],
            total_difficulty,
        };

        // Create the block builder, run the transactions and extract the DB
        Self::preflight_with_db(chain_spec, preflight_db, data, ommers)
    }

    fn preflight_with_db(
        chain_spec: Arc<ChainSpec>,
        preflight_db: PreflightDB,
        data: StatelessClientData<Block, Header>,
        ommers: Vec<Vec<Header>>,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        // Instantiate the engine with a rescue for the DB
        info!("Running block execution engine ...");
        let mut engine = StatelessClientEngine::<B, H, PreflightDB, R>::new(
            chain_spec,
            StatelessClientData::<B, H>::derive(data.clone(), ommers.clone()),
            Some(preflight_db),
        );

        let block_count = data.blocks.len();

        let mut state_trie = MptNode::from(data.parent_header.state_root);
        let mut storage_tries = Default::default();
        let mut contracts: HashSet<Bytecode> = Default::default();
        let mut ancestor_headers: Vec<Header> = Default::default();

        for _ in 0..block_count {
            // Run the engine
            info!("Pre execution validation ...");
            engine
                .pre_execution_validation::<<Self as PreflightClient<B, H, R>>::PreExecValidation>(
                )?;
            info!("Executing transactions ...");
            let execution_output = engine
                .execute_transactions::<<Self as PreflightClient<B, H, R>>::TransactionExecution>(
            )?;
            info!("Post execution validation ...");
            let bundle_state = engine
                .post_execution_validation::<<Self as PreflightClient<B, H, R>>::PostExecValidation>(
                    execution_output,
                )?;
            let state_changeset = bundle_state.into_plain_state(OriginalValuesKnown::Yes);
            info!("Provider-backed execution is Done!");

            // Rescue the dropped DB and apply the state changeset
            let mut preflight_db = engine.db.take().unwrap().unwrap();
            preflight_db.apply_changeset(state_changeset)?;

            // storage sanity check
            // {
            //     let init_db = preflight_db.db.db.db.borrow_mut();
            //     let mut provider_db = init_db.db.clone();
            //     provider_db.block_no += 1;
            //     for (address, db_account) in &preflight_db.db.accounts {
            //         use reth_revm::Database;
            //         let provider_info = provider_db.basic(*address)?.unwrap();
            //         if db_account.info != provider_info {
            //             dbg!(&address);
            //             dbg!(&db_account.info);
            //             dbg!(&provider_info);
            //         }
            //     }
            // }

            // Save the provider cache
            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // Gather inclusion proofs for the initial and final state
            info!("Gathering initial proofs ...");
            let initial_proofs = preflight_db.get_initial_proofs()?;
            info!("Saving provider cache ...");
            preflight_db.save_provider()?;
            info!("Gathering final proofs ...");
            let latest_proofs = preflight_db.get_latest_proofs()?;
            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // Gather proofs for block history
            info!("Gathering ancestor headers ...");
            let new_ancestor_headers = preflight_db.get_ancestor_headers()?;
            if ancestor_headers.is_empty()
                || (!new_ancestor_headers.is_empty()
                    && new_ancestor_headers.last().unwrap().number
                        < ancestor_headers.last().unwrap().number)
            {
                let _ = replace(&mut ancestor_headers, new_ancestor_headers);
            }

            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // collect the code from each account
            info!("Collecting contracts ...");
            let initial_db = preflight_db.inner.db.db.borrow();
            for account in initial_db.accounts.values() {
                let code = account.info.code.clone().context("missing code")?;
                if !code.is_empty() {
                    contracts.insert(code);
                }
            }
            drop(initial_db);

            // construct the sparse MPTs from the inclusion proofs
            info!(
                "Extending tries from {} initialization and {} finalization proofs ...",
                initial_proofs.len(),
                latest_proofs.len()
            );
            extend_proof_tries(
                &mut state_trie,
                &mut storage_tries,
                initial_proofs,
                latest_proofs,
            )?;

            // Increment block number counter
            preflight_db.advance_provider_block()?;

            // Give db back to engine
            engine.replace_db(Wrapper::from(preflight_db))?;

            // Advance engine manually
            engine.data.parent_header = R::block_to_header(engine.data.blocks.pop().unwrap());
            engine.data.total_difficulty =
                R::accumulate_difficulty(engine.data.total_difficulty, &engine.data.parent_header);

            // Report stats
            info!("State trie: {} nodes", state_trie.size());
            let storage_nodes: u64 = storage_tries
                .iter()
                .map(|(_, (n, _))| n.size() as u64)
                .sum();
            info!(
                "Storage tries: {storage_nodes} total nodes over {} accounts",
                storage_tries.len()
            );
        }
        info!("Blocks: {}", data.blocks.len());
        let transactions: u64 = data
            .blocks
            .iter()
            .map(|b| b.transactions.len() as u64)
            .sum();
        info!("Transactions: {transactions} total transactions");

        Ok(StatelessClientData::<B, H> {
            blocks: zip(data.blocks, ommers)
                .map(|(block, ommers)| B::derive(block, ommers))
                .collect(),
            state_trie,
            storage_tries,
            contracts: contracts.into_iter().map(|b| b.bytes()).collect(),
            parent_header: H::derive(data.parent_header),
            ancestor_headers: ancestor_headers.into_iter().map(|h| H::derive(h)).collect(),
            total_difficulty: data.total_difficulty,
        })
    }
}

pub struct RethPreflightClient;

impl PreflightClient<reth_primitives::Block, reth_primitives::Header, RethDriver>
    for RethPreflightClient
{
    type PreExecValidation = RethPreExecStrategy;
    type TransactionExecution = RethExecStrategy;
    type PostExecValidation = RethPostExecStrategy;
}
