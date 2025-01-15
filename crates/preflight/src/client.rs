// Copyright 2024, 2025 RISC Zero, Inc.
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

use crate::db::PreflightDB;
use crate::driver::PreflightDriver;
use crate::provider::db::ProviderDB;
use crate::provider::query::{BlockQuery, UncleQuery};
use crate::provider::{new_provider, Provider};
use crate::trie::extend_proof_tries;
use alloy::network::Network;
use alloy::primitives::map::{AddressHashMap, HashMap};
use alloy::primitives::Bytes;
use anyhow::Context;
use log::{debug, info, warn};
use std::cell::RefCell;
use std::iter::zip;
use std::path::PathBuf;
use std::rc::Rc;
use zeth_core::db::update::into_plain_state;
use zeth_core::driver::CoreDriver;
use zeth_core::mpt::{
    parse_proof, resolve_nodes_in_place, shorten_node_path, MptNode, MptNodeReference,
};
use zeth_core::rescue::Wrapper;
use zeth_core::stateless::data::{StatelessClientData, StorageEntry};
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
        Self::preflight_with_provider(provider, block_no, block_count)
    }

    fn preflight_with_provider(
        provider: Rc<RefCell<dyn Provider<N>>>,
        block_no: u64,
        block_count: u64,
    ) -> anyhow::Result<StatelessClientData<R::Block, R::Header>> {
        let mut provider_mut = provider.borrow_mut();
        let chain = provider_mut.get_chain()?;
        let chain_spec = R::chain_spec(&chain).expect("Unsupported chain");
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
        provider_mut.reset(block_no)?;
        drop(provider_mut);
        let provider_db =
            ProviderDB::<N, R, P>::new(provider, R::block_number(&core_parent_header));
        let preflight_db = PreflightDB::from(provider_db);

        // Create the input data
        let total_difficulty = P::total_difficulty(&parent_header).unwrap_or_default();
        if total_difficulty.is_zero() {
            warn!("Provider reported a total chain difficulty value of zero.")
        }
        let final_difficulty = R::final_difficulty(
            R::block_number(&core_parent_header),
            total_difficulty,
            chain_spec.as_ref(),
        );
        if final_difficulty.is_zero() {
            warn!("Proving a final chain difficulty value of zero.")
        }

        let data = StatelessClientData {
            chain,
            blocks: blocks.into_iter().rev().collect(),
            signers: Default::default(),
            state_trie: Default::default(),
            storage_tries: Default::default(),
            contracts: Default::default(),
            parent_header,
            ancestor_headers: vec![],
            total_difficulty: final_difficulty,
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
        let mut storage_tries = AddressHashMap::<StorageEntry>::default();
        let mut contracts: Vec<Bytes> = Default::default();
        let mut ancestor_headers: Vec<R::Header> = Default::default();

        for num_blocks in 1..=block_count {
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
            preflight_db.apply_changeset(state_changeset.clone())?;

            // Save the provider cache
            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // info!("Sanity check ...");
            // preflight_db.sanity_check(state_changeset)?;
            // preflight_db.save_provider()?;

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
            preflight_db
                .get_ancestor_headers()?
                .into_iter()
                .map(|h| P::derive_header(h))
                .for_each(|new_ancestor_header| {
                    let earliest_header = ancestor_headers.last().unwrap_or(&core_parent_header);
                    if R::block_number(&new_ancestor_header) == R::block_number(earliest_header) - 1
                    {
                        ancestor_headers.push(new_ancestor_header);
                    }
                });
            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // collect the code of the used contracts
            let initial_db = preflight_db.inner.db.db.borrow();
            for code in initial_db.contracts.values() {
                contracts.push(code.bytes().clone());
            }
            drop(initial_db);
            info!("Collected contracts: {}", contracts.len());

            // construct the sparse MPTs from the inclusion proofs
            info!(
                "Extending tries from {} initialization and {} finalization proofs ...",
                initial_proofs.len(),
                latest_proofs.len()
            );
            let (state_orphans, storage_orphans) = extend_proof_tries(
                &mut state_trie,
                &mut storage_tries,
                initial_proofs,
                latest_proofs,
            )
            .context("failed to extend proof tries")?;
            // resolve potential orphans
            let orphan_resolves =
                preflight_db.resolve_orphans(block_count as u64, &state_orphans, &storage_orphans);
            info!(
                "Using {} proofs to resolve {} state and {} storage orphans.",
                orphan_resolves.len(),
                state_orphans.len(),
                storage_orphans.len()
            );
            for account_proof in orphan_resolves {
                let node_store = parse_proof(&account_proof.account_proof)?
                    .iter()
                    .flat_map(|n| {
                        vec![vec![n.clone()], shorten_node_path(n)]
                            .into_iter()
                            .flatten()
                    })
                    .map(|n| (n.reference().as_digest(), n))
                    .collect();
                resolve_nodes_in_place(&mut state_trie, &node_store);
                // resolve storage orphans
                if let Some(StorageEntry { storage_trie, .. }) =
                    storage_tries.get_mut(&account_proof.address)
                {
                    for storage_proof in account_proof.storage_proof {
                        let node_store: HashMap<MptNodeReference, MptNode> =
                            parse_proof(&storage_proof.proof)?
                                .iter()
                                .flat_map(|n| {
                                    vec![vec![n.clone()], shorten_node_path(n)]
                                        .into_iter()
                                        .flatten()
                                })
                                .map(|n| (n.reference().as_digest(), n))
                                .collect();
                        for k in node_store.keys() {
                            let digest = k.digest();
                            if storage_orphans
                                .iter()
                                .any(|(a, (_, d))| a == &account_proof.address && &digest == d)
                            {
                                info!(
                                    "Resolved storage node {digest} for account {}",
                                    account_proof.address
                                );
                            }
                        }
                        resolve_nodes_in_place(storage_trie, &node_store);
                    }
                }
            }

            info!("Saving provider cache ...");
            preflight_db.save_provider()?;

            // Increment block number counter
            preflight_db.advance_provider_block()?;
            preflight_db.clear()?;

            // Give db back to engine
            engine.replace_db(Wrapper::from(preflight_db))?;

            // Advance engine manually
            engine.data.parent_header = R::block_to_header(engine.data.blocks.pop().unwrap());
            engine.data.signers.pop();
            engine.data.total_difficulty =
                R::accumulate_difficulty(engine.data.total_difficulty, &engine.data.parent_header);

            // Report stats
            info!("State trie: {} nodes", state_trie.size());
            let storage_nodes: u64 = storage_tries
                .values()
                .map(|e| e.storage_trie.size() as u64)
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

        let blocks: Vec<_> = zip(data.blocks, ommers)
            .map(|(block, ommers)| P::derive_block(block, ommers))
            .collect();
        let signers = blocks.iter().map(P::recover_signers).collect();
        Ok(StatelessClientData {
            chain: data.chain,
            blocks,
            signers,
            state_trie,
            storage_tries,
            contracts,
            parent_header: P::derive_header(data.parent_header),
            ancestor_headers,
            total_difficulty: data.total_difficulty,
        })
    }
}
