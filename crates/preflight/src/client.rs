use crate::db::PreflightDB;
use crate::driver::PreflightDriver;
use crate::provider::db::ProviderDB;
use crate::provider::new_provider;
use crate::provider::query::{BlockQuery, UncleQuery};
use crate::trie::extend_proof_tries;
use alloy::network::Network;
use anyhow::Context;
use log::{debug, info};
use std::iter::zip;
use std::mem::replace;
use std::path::PathBuf;
use zeth_core::db::into_plain_state;
use zeth_core::driver::CoreDriver;
use zeth_core::mpt::MptNode;
use zeth_core::rescue::Wrapper;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core::stateless::engine::StatelessClientEngine;
use zeth_core::stateless::execute::ExecutionStrategy;
use zeth_core::stateless::validate::ValidationStrategy;

pub trait PreflightClient<N: Network, R: CoreDriver, P: PreflightDriver<R, N>>
where
    R: Clone,
    P: Clone,
{
    type Validation: ValidationStrategy<R, PreflightDB<N, R, P>>;
    type Execution: ExecutionStrategy<R, Wrapper<PreflightDB<N, R, P>>>;

    fn preflight(
        chain_id: Option<u64>,
        cache_dir: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
        block_count: u64,
    ) -> anyhow::Result<StatelessClientData<R::Block, R::Header>> {
        let provider = new_provider::<N>(cache_dir.clone(), block_no, rpc_url.clone(), chain_id)?;
        let mut provider_mut = provider.borrow_mut();
        let chain = provider_mut.get_chain()?;
        // Fetch the parent block
        let parent_block = provider_mut.get_full_block(&BlockQuery {
            block_no: block_no - 1,
        })?;
        let parent_header = P::derive_header_response(parent_block);
        let core_parent_header = P::derive_header(parent_header.clone());
        debug!(
            "Initial block: {:?} ({:?})",
            R::block_number(&core_parent_header),
            R::header_hash(&core_parent_header)
        );

        // Fetch the blocks and their uncles
        info!("Grabbing blocks and their uncles ...");
        let mut blocks = Vec::new();
        let mut ommers = Vec::new();
        for block_no in block_no..block_no + block_count {
            let block = provider_mut.get_full_block(&BlockQuery { block_no })?;
            let uncle_headers: Vec<_> = P::uncles(&block)
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    P::derive_header_response(
                        provider_mut
                            .get_uncle_block(&UncleQuery {
                                block_no,
                                uncle_index: idx as u64,
                            })
                            .expect("Failed to retrieve uncle block"),
                    )
                })
                .collect();
            // Print Debug info
            let core_block_header = P::derive_header(P::header_response(&block).clone());
            debug!(
                "Block number: {:?} ({:?})",
                R::block_number(&core_block_header),
                R::header_hash(&core_block_header),
            );
            debug!("Transaction count: {:?}", P::count_transactions(&block));
            debug!("Uncle count: {:?}", P::uncles(&block).len());
            // Collect data
            blocks.push(block);
            ommers.push(uncle_headers);
            // Prepare for next iteration
            provider_mut.save()?;
            provider_mut.advance()?;
        }
        ommers.reverse();

        // Create the provider DB with a fresh provider to reset block_no
        let provider_db = ProviderDB::<N, R, P>::new(
            new_provider::<N>(cache_dir, block_no, rpc_url, chain_id)?,
            R::block_number(&core_parent_header),
        );
        let preflight_db = PreflightDB::from(provider_db);

        // Create the input data
        let total_difficulty = P::total_difficulty(&parent_header).unwrap_or_default();
        let data = StatelessClientData {
            chain,
            blocks: blocks.into_iter().rev().collect(),
            state_trie: Default::default(),
            storage_tries: Default::default(),
            contracts: Default::default(),
            parent_header,
            ancestor_headers: vec![],
            total_difficulty,
        };

        // Create the block builder, run the transactions and extract the DB
        Self::preflight_with_db(preflight_db, data, ommers)
    }

    fn preflight_with_db(
        preflight_db: PreflightDB<N, R, P>,
        data: StatelessClientData<N::BlockResponse, N::HeaderResponse>,
        ommers: Vec<Vec<N::HeaderResponse>>,
    ) -> anyhow::Result<StatelessClientData<R::Block, R::Header>> {
        // Instantiate the engine with a rescue for the DB
        info!("Running block execution engine ...");
        let mut engine = StatelessClientEngine::<R, PreflightDB<N, R, P>>::new(
            P::derive_data(data.clone(), ommers.clone()),
            Some(preflight_db),
        );

        let block_count = data.blocks.len();

        let core_parent_header = P::derive_header(data.parent_header.clone());
        let mut state_trie = MptNode::from(R::state_root(&core_parent_header));
        let mut storage_tries = Default::default();
        let mut contracts = data.contracts.clone();
        let mut ancestor_headers: Vec<R::Header> = Default::default();

        for num_blocks in 0..block_count {
            // Run the engine
            info!("Pre execution validation ...");
            engine.validate_header::<<Self as PreflightClient<N, R, P>>::Validation>()?;
            info!("Executing transactions ...");
            let bundle_state =
                engine.execute_transactions::<<Self as PreflightClient<N, R, P>>::Execution>()?;
            let state_changeset = into_plain_state(bundle_state);
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
            let new_ancestor_headers: Vec<_> = preflight_db
                .get_ancestor_headers()?
                .into_iter()
                .map(|h| P::derive_header(h))
                .collect();
            if ancestor_headers.is_empty()
                || (!new_ancestor_headers.is_empty()
                    && R::block_number(new_ancestor_headers.last().unwrap())
                        < R::block_number(ancestor_headers.last().unwrap()))
            {
                let _ = replace(&mut ancestor_headers, new_ancestor_headers);
            }

            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // collect the code from each account
            info!("Collecting contracts ...");
            let initial_db = preflight_db.inner.db.db.borrow();
            for (address, account) in initial_db.accounts.iter() {
                let code = account.info.code.clone().context("missing code")?;
                if !code.is_empty() && !contracts.contains_key(address) {
                    contracts.insert(*address, code.bytes());
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
            preflight_db.clear()?;

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
            info!("Witness now covers {num_blocks} blocks.");
        }
        let transactions: u64 = data
            .blocks
            .iter()
            .map(|b| P::count_transactions(b) as u64)
            .sum();
        info!("{transactions} total transactions.");

        Ok(StatelessClientData::<R::Block, R::Header> {
            chain: data.chain,
            blocks: zip(data.blocks, ommers)
                .map(|(block, ommers)| P::derive_block(block, ommers))
                .collect(),
            state_trie,
            storage_tries,
            contracts,
            parent_header: P::derive_header(data.parent_header),
            ancestor_headers,
            total_difficulty: data.total_difficulty,
        })
    }
}
