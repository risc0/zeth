use crate::db::PreflightDB;
use crate::derive::{RPCDerivableBlock, RPCDerivableData, RPCDerivableHeader};
use crate::provider::db::ProviderDB;
use crate::provider::{new_provider, BlockQuery};
use crate::trie::proofs_to_tries;
use alloy::rpc::types::{Block, Header};
use anyhow::Context;
use hashbrown::HashSet;
use log::{debug, info};
use reth_chainspec::ChainSpec;
use reth_revm::db::{BundleState, OriginalValuesKnown};
use std::path::PathBuf;
use std::sync::Arc;
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
    type TransactionExecution: for<'a> TransactionExecutionStrategy<
        B,
        H,
        PreflightDB,
        Input<'a> = DbExecutionInput<'a, B, PreflightDB>,
    >;
    type PostExecValidation: for<'a, 'b> PostExecutionValidationStrategy<
        B,
        H,
        PreflightDB,
        Input<'a> = <Self::TransactionExecution as TransactionExecutionStrategy<
            B,
            H,
            PreflightDB,
        >>::Output<'a>,
        Output<'b> = BundleState,
    >;

    fn preflight_with_rpc(
        chain_spec: Arc<ChainSpec>,
        cache_path: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        let provider = new_provider(cache_path, rpc_url)?;
        // Fetch the parent block
        let parent_block = provider.borrow_mut().get_full_block(&BlockQuery {
            block_no: block_no - 1,
        })?;
        debug!(
            "Initial block: {:?} ({:?})",
            parent_block.header.number, parent_block.header.hash
        );
        let parent_header = parent_block.header;

        // Fetch the target block
        let block = provider
            .borrow_mut()
            .get_full_block(&BlockQuery { block_no })?;

        debug!(
            "Final block number: {:?} ({:?})",
            block.header.number, block.header.hash,
        );
        debug!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = ProviderDB::new(provider, parent_header.number);
        let preflight_db = PreflightDB::from(provider_db);

        // Create the input data
        let total_difficulty = parent_header
            .total_difficulty
            .expect("Missing total difficulty");
        let data = StatelessClientData {
            block,
            parent_state_trie: Default::default(),
            parent_storage: Default::default(),
            contracts: vec![],
            parent_header,
            ancestor_headers: vec![],
            total_difficulty,
        };

        // Create the block builder, run the transactions and extract the DB
        Self::preflight_with_db(chain_spec, preflight_db, data)
    }

    fn preflight_with_db(
        chain_spec: Arc<ChainSpec>,
        mut preflight_db: PreflightDB,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        let preflight_db_rescue = preflight_db.get_rescue();
        info!("Grabbing uncles ...");
        let ommers = preflight_db.get_uncles(&data.block.uncles)?;
        // Instantiate the engine with a rescue for the DB
        info!("Running block execution engine ...");
        let mut engine = StatelessClientEngine::<B, H, PreflightDB, R>::new(
            chain_spec,
            StatelessClientData::<B, H>::derive(data.clone(), ommers.clone()),
            Some(preflight_db),
        );
        // Run the engine
        info!("Pre execution validation ...");
        engine
            .pre_execution_validation::<<Self as PreflightClient<B, H, R>>::PreExecValidation>()?;
        info!("Executing transactions ...");
        let execution_output = engine
            .execute_transactions::<<Self as PreflightClient<B, H, R>>::TransactionExecution>()?;
        info!("Post execution validation ...");
        let bundle_state =
            StatelessClientEngine::<B, H, PreflightDB, R>::post_execution_validation::<
                <Self as PreflightClient<B, H, R>>::PostExecValidation,
            >(execution_output)?;
        let state_changeset = bundle_state.into_plain_state(OriginalValuesKnown::Yes);
        info!("Provider-backed execution is Done!");

        // Rescue the dropped DB and apply the state changeset
        let mut preflight_db = PreflightDB::from(preflight_db_rescue);
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
        let ancestor_headers = preflight_db.get_ancestor_headers()?;
        info!("Saving provider cache ...");
        preflight_db.save_provider()?;

        // collect the code from each account
        info!("Collecting contracts ...");
        let mut contracts = HashSet::new();
        let initial_db = &preflight_db.db.db.db.borrow();
        for account in initial_db.accounts.values() {
            let code = account.info.code.clone().context("missing code")?;
            if !code.is_empty() {
                contracts.insert(code);
            }
        }

        // construct the sparse MPTs from the inclusion proofs
        info!("Deriving tries from proofs ...");
        let (parent_state_trie, parent_storage) =
            proofs_to_tries(data.parent_header.state_root, initial_proofs, latest_proofs)?;

        info!(
            "The partial state trie consists of {} nodes",
            parent_state_trie.size()
        );
        info!(
            "The partial storage tries consist of {} nodes",
            parent_storage
                .values()
                .map(|(n, _)| n.size())
                .sum::<usize>()
        );

        Ok(StatelessClientData::<B, H> {
            block: B::derive(data.block, ommers),
            parent_state_trie,
            parent_storage,
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
