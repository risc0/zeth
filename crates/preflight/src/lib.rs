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

use crate::db::{
    get_ancestor_headers, get_initial_proofs, get_latest_proofs, get_uncles, PreflightDb,
};
use crate::derive::{RPCDerivableBlock, RPCDerivableData, RPCDerivableHeader};
use crate::provider::db::ProviderDb;
use crate::provider::{new_provider, BlockQuery};
use crate::trie::proofs_to_tries;
use alloy::primitives::U256;
use alloy::rpc::types::{Block, Header};
use anyhow::Context;
use hashbrown::HashSet;
use log::{debug, info};
use reth_chainspec::ChainSpec;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zeth_core::stateless::client::StatelessClientEngine;
use zeth_core::stateless::data::StatelessClientData;
use zeth_core::stateless::execute::TransactionExecutionStrategy;
use zeth_core::stateless::post_exec::PostExecutionValidationStrategy;
use zeth_core::stateless::pre_exec::PreExecutionValidationStrategy;

pub mod db;
pub mod derive;
pub mod provider;
pub mod trie;

pub trait PreflightClient<B, H> {
    /// Executes the complete block using the input and state from the RPC provider.
    /// It returns all the data required to build and validate the block.
    fn preflight_with_rpc(
        chain_spec: Arc<ChainSpec>,
        cache_path: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> anyhow::Result<StatelessClientData<B, H>>;

    fn preflight_with_db(
        chain_spec: Arc<ChainSpec>,
        preflight_db: PreflightDb,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<StatelessClientData<B, H>>;
}

impl<T, B: RPCDerivableBlock, H: RPCDerivableHeader> PreflightClient<B, H> for T
where
    T: PreExecutionValidationStrategy<B, H, PreflightDb>
        + TransactionExecutionStrategy<
            B,
            H,
            PreflightDb,
            Output = <T as PostExecutionValidationStrategy<B, H, PreflightDb>>::Input,
        > + PostExecutionValidationStrategy<B, H, PreflightDb>,
    StatelessClientData<B, H>: RPCDerivableData,
{
    fn preflight_with_rpc(
        chain_spec: Arc<ChainSpec>,
        cache_path: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        let mut provider = new_provider(cache_path, rpc_url)?;
        // Fetch the parent block
        let parent_block = provider.get_full_block(&BlockQuery {
            block_no: block_no - 1,
        })?;
        debug!(
            "Initial block: {:?} ({:?})",
            parent_block.header.number, parent_block.header.hash
        );
        let parent_header = parent_block.header;

        // Fetch the target block
        let block = provider.get_full_block(&BlockQuery { block_no })?;

        debug!(
            "Final block number: {:?} ({:?})",
            block.header.number, block.header.hash,
        );
        debug!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = ProviderDb::new(provider, parent_header.number);
        let preflight_db = PreflightDb::from(provider_db);

        // Create the input data
        let data = StatelessClientData {
            block,
            parent_state_trie: Default::default(),
            parent_storage: Default::default(),
            contracts: vec![],
            parent_header,
            ancestor_headers: vec![],
        };

        // Create the block builder, run the transactions and extract the DB
        Self::preflight_with_db(chain_spec, preflight_db, data)
    }

    fn preflight_with_db(
        chain_spec: Arc<ChainSpec>,
        mut preflight_db: PreflightDb,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        info!("Grabbing uncles ...");
        let ommers = get_uncles(&mut preflight_db, &data.block.uncles)?;
        // Instantiate the engine with a rescue for the DB
        info!("Running block execution engine ...");
        let preflight_db_rescue = Arc::new(Mutex::new(None));
        let mut engine = StatelessClientEngine::new(
            chain_spec,
            StatelessClientData::<B, H>::derive(data.clone(), ommers.clone()),
            U256::ZERO, // todo query for correct total difficulty
            Some(preflight_db),
            Some(preflight_db_rescue.clone()),
        );
        // Run the engine and extract the DB when its dropped even on failure
        if let Ok(_) = engine.pre_execution_validation::<Self>() {
            if let Ok(execution_output) = engine.execute_transactions::<Self>() {
                let _ = engine.post_execution_validation::<Self>(execution_output);
            }
        }
        let mut preflight_db = preflight_db_rescue.lock().unwrap().take().unwrap();

        // Gather inclusion proofs for the initial and final state
        info!("Gathering proofs ...");
        let parent_proofs = get_initial_proofs(&mut preflight_db)?;
        let latest_proofs = get_latest_proofs(&mut preflight_db)?;

        // Gather proofs for block history
        let ancestor_headers = get_ancestor_headers(&mut preflight_db)?;

        // Save the provider cache
        info!("Saving provider cache ...");
        preflight_db.db.db.save_provider()?;

        info!("Provider-backed execution is Done!");

        // collect the code from each account
        let mut contracts = HashSet::new();
        let initial_db = &preflight_db.db;
        for account in initial_db.accounts.values() {
            let code = account.info.code.clone().context("missing code")?;
            if !code.is_empty() {
                contracts.insert(code);
            }
        }

        // construct the sparse MPTs from the inclusion proofs
        let (parent_state_trie, parent_storage) =
            proofs_to_tries(data.parent_header.state_root, parent_proofs, latest_proofs)?;

        debug!(
            "The partial state trie consists of {} nodes",
            parent_state_trie.size()
        );
        debug!(
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
        })
    }
}
