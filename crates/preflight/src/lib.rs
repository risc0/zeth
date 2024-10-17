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

use crate::db::PreflightDb;
use crate::provider::db::ProviderDb;
use crate::provider::{new_provider, BlockQuery};
use alloy::primitives::U256;
use alloy::rpc::types::{Block, Header};
use log::debug;
use reth_chainspec::ChainSpec;
use reth_revm::InMemoryDB;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zeth_core::stateless::client::{StatelessClient, StatelessClientEngine};
use zeth_core::stateless::data::StatelessClientData;

pub mod db;
pub mod provider;
pub mod trie;

// /// The initial data required to build a block as returned by the [Preflight].
// #[derive(Debug, Clone)]
// pub struct Data<E: TxEssence> {
//     pub db: MemDb,
//     pub parent_header: Header,
//     pub parent_proofs: HashMap<Address, EIP1186ProofResponse>,
//     pub header: Option<Header>,
//     pub transactions: Vec<Transaction<E>>,
//     pub withdrawals: Vec<Withdrawal>,
//     pub proofs: HashMap<Address, EIP1186ProofResponse>,
//     pub ancestor_headers: Vec<Header>,
// }

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

impl<T, B, H> PreflightClient<B, H> for T
where
    T: StatelessClient<B, H, InMemoryDB>,
    StatelessClientData<B, H>: From<StatelessClientData<Block, Header>>,
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
        preflight_db: PreflightDb,
        data: StatelessClientData<Block, Header>,
    ) -> anyhow::Result<StatelessClientData<B, H>> {
        let parent_header = data.parent_header.clone();
        let transactions = data.block.transactions.clone();
        let withdrawals = data.block.withdrawals.clone();
        // Run the engine and extract the DB even if run fails
        let db_backup = Arc::new(Mutex::new(None));
        let mut engine = StatelessClientEngine::<B, H, PreflightDb>::new(
            chain_spec,
            data.into(),
            U256::ZERO, // todo query for correct total difficulty
            Some(preflight_db),
            Some(db_backup.clone()),
        );
        // todo: take db from engine
        // let mut preflight_db = match engine.pre_execution_validation() {
        //     Ok(_) => match engine.execute_transactions() {
        //         Ok(_) => engine.take_db().unwrap(),
        //         Err(_) => db_backup.lock().unwrap().take().unwrap(),
        //     },
        //     Err(_) => db_backup.lock().unwrap().take().unwrap(),
        // };

        // let x: alloy::consensus::Block<alloy::consensus::TypedTransaction> = data.block.into();

        todo!()
    }
}
