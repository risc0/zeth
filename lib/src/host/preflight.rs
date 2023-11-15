// Copyright 2023 RISC Zero, Inc.
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

use std::{fmt::Debug, iter::once};

use anyhow::{anyhow, Context, Result};
use ethers_core::types::{
    Block as EthersBlock, Bytes as EthersBytes, EIP1186ProofResponse,
    Transaction as EthersTransaction,
};
use hashbrown::{HashMap, HashSet};
use log::info;
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    transactions::{Transaction, TxEssence},
    trie::{MptNode, MptNodeData, MptNodeReference, EMPTY_ROOT},
    withdrawal::Withdrawal,
    Address,
};

use crate::{
    builder::{BlockBuilder, BlockBuilderStrategy},
    consts::ChainSpec,
    host::{
        mpt::{orphaned_digests, resolve_digests, shorten_key},
        provider::{new_provider, BlockQuery},
    },
    input::{Input, StorageEntry},
    mem_db::MemDb,
};

/// The initial data required to build a block as returned by the [Preflight].
#[derive(Debug, Clone)]
pub struct Data<E: TxEssence> {
    pub db: MemDb,
    pub parent_block: Header,
    pub parent_proofs: HashMap<Address, EIP1186ProofResponse>,
    pub block: Header,
    pub transactions: Vec<Transaction<E>>,
    pub withdrawals: Vec<Withdrawal>,
    pub proofs: HashMap<Address, EIP1186ProofResponse>,
    pub ancestor_headers: Vec<Header>,
}

pub trait Preflight<E: TxEssence> {
    /// Executes the complete block using the input and state from the RPC provider.
    /// It returns all the data required to build and validate the block.
    fn run_preflight(
        chain_spec: ChainSpec,
        cache_path: Option<String>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> Result<Data<E>>;
}

/// Implements the [Preflight] trait for all compatible [BlockBuilderStrategy]s.
impl<N: BlockBuilderStrategy> Preflight<N::TxEssence> for N
where
    N::TxEssence: TryFrom<EthersTransaction>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    fn run_preflight(
        chain_spec: ChainSpec,
        cache_path: Option<String>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> Result<Data<N::TxEssence>> {
        let mut provider = new_provider(cache_path, rpc_url)?;

        // Fetch the parent block
        let parent_block = provider.get_partial_block(&BlockQuery {
            block_no: block_no - 1,
        })?;

        info!(
            "Initial block: {:?} ({:?})",
            parent_block.number.unwrap(),
            parent_block.hash.unwrap()
        );

        // Fetch the target block
        let block = provider.get_full_block(&BlockQuery { block_no })?;

        info!(
            "Final block number: {:?} ({:?})",
            block.number.unwrap(),
            block.hash.unwrap()
        );
        info!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = crate::host::provider_db::ProviderDb::new(
            provider,
            parent_block.number.unwrap().as_u64(),
        );

        // Create the input data
        let input = new_preflight_input(block.clone(), parent_block.clone()).context("context")?;
        let transactions = input.transactions.clone();
        let withdrawals = input.withdrawals.clone();

        // Create the block builder, run the transactions and extract the DB
        let mut builder = BlockBuilder::new(&chain_spec, input)
            .with_db(provider_db)
            .prepare_header::<N::HeaderPrepStrategy>()?
            .execute_transactions::<N::TxExecStrategy>()?;
        let provider_db = builder.mut_db().unwrap();

        info!("Gathering inclusion proofs ...");

        // Gather inclusion proofs for the initial and final state
        let parent_proofs = provider_db.get_initial_proofs()?;
        let proofs = provider_db.get_latest_proofs()?;

        // Gather proofs for block history
        let ancestor_headers = provider_db.get_ancestor_headers()?;

        info!("Saving provider cache ...");

        // Save the provider cache
        provider_db.get_provider().save()?;

        info!("Provider-backed execution is Done!");

        Ok(Data {
            db: provider_db.get_initial_db().clone(),
            parent_block: parent_block.try_into()?,
            parent_proofs,
            block: block.try_into()?,
            transactions,
            withdrawals,
            proofs,
            ancestor_headers,
        })
    }
}

fn new_preflight_input<E, T>(
    block: EthersBlock<EthersTransaction>,
    parent_block: EthersBlock<T>,
) -> Result<Input<E>>
where
    E: TxEssence + TryFrom<EthersTransaction>,
    <E as TryFrom<EthersTransaction>>::Error: Debug,
{
    // convert each transaction
    let transactions = block
        .transactions
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.try_into()
                .map_err(|err| anyhow!("transaction {i} invalid: {err:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // convert each withdrawal
    let withdrawals = block
        .withdrawals
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            tx.try_into()
                .with_context(|| format!("withdrawal {i} invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let input = Input {
        beneficiary: from_ethers_h160(block.author.context("author missing")?),
        gas_limit: from_ethers_u256(block.gas_limit),
        timestamp: from_ethers_u256(block.timestamp),
        extra_data: block.extra_data.0.into(),
        mix_hash: from_ethers_h256(block.mix_hash.context("mix_hash missing")?),
        transactions,
        withdrawals,
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        parent_header: parent_block.try_into().context("invalid parent block")?,
        ancestor_headers: Default::default(),
    };
    Ok(input)
}

impl<E: TxEssence> From<Data<E>> for Input<E> {
    fn from(data: Data<E>) -> Input<E> {
        // construct the proof tries
        let (mut nodes_by_reference, mut storage) =
            proofs_to_tries(data.parent_proofs.values().cloned().collect());
        // there should be a trie and a list of storage slots for every account
        assert_eq!(storage.len(), data.db.accounts_len());

        // collect the code from each account
        let mut contracts = HashMap::new();
        for account in data.db.accounts.values() {
            let code = account.info.code.clone().unwrap();
            if !code.is_empty() {
                contracts.insert(code.hash_slow(), code.bytecode);
            }
        }

        // extract the state trie
        let state_root = data.parent_block.state_root;
        let state_trie = nodes_by_reference
            .remove(&MptNodeReference::Digest(state_root))
            .expect("State root node not found");
        assert_eq!(state_root, state_trie.hash());

        // identify orphaned digests, that could lead to issues when deleting nodes
        let mut orphans = HashSet::new();
        for root in storage.values().map(|v| &v.0).chain(once(&state_trie)) {
            let root = resolve_digests(root, &nodes_by_reference);
            orphans.extend(orphaned_digests(&root));
        }
        // resolve those orphans using the proofs of the final state
        for fini_proof in data.proofs.values() {
            resolve_orphans(
                &fini_proof.account_proof,
                &mut orphans,
                &mut nodes_by_reference,
            );
            for storage_proof in &fini_proof.storage_proof {
                resolve_orphans(&storage_proof.proof, &mut orphans, &mut nodes_by_reference);
            }
        }

        // resolve the pointers in the state root node and all storage root nodes
        let state_trie = resolve_digests(&state_trie, &nodes_by_reference);
        storage
            .values_mut()
            .for_each(|(n, _)| *n = resolve_digests(n, &nodes_by_reference));

        info!(
            "The partial state trie consists of {} nodes",
            state_trie.size()
        );
        info!(
            "The partial storage tries consist of {} nodes",
            storage.values().map(|(n, _)| n.size()).sum::<usize>()
        );

        // Create the block builder input
        Input {
            parent_header: data.parent_block,
            beneficiary: data.block.beneficiary,
            gas_limit: data.block.gas_limit,
            timestamp: data.block.timestamp,
            extra_data: data.block.extra_data.0.clone().into(),
            mix_hash: data.block.mix_hash,
            transactions: data.transactions,
            withdrawals: data.withdrawals,
            parent_state_trie: state_trie,
            parent_storage: storage.into_iter().collect(),
            contracts: contracts.into_values().collect(),
            ancestor_headers: data.ancestor_headers,
        }
    }
}

fn proofs_to_tries(
    proofs: Vec<EIP1186ProofResponse>,
) -> (
    HashMap<MptNodeReference, MptNode>,
    HashMap<Address, StorageEntry>,
) {
    // construct the proof tries
    let mut nodes_by_reference = HashMap::new();
    let mut storage = HashMap::new();
    for proof in proofs {
        // parse the nodes of the account proof
        for bytes in &proof.account_proof {
            let mpt_node = MptNode::decode(bytes).expect("Failed to decode state proof");
            nodes_by_reference.insert(mpt_node.reference(), mpt_node);
        }

        // process the proof for each storage entry
        let mut root_node = None;
        for storage_proof in &proof.storage_proof {
            // parse the nodes of the storage proof and return the root node
            root_node = storage_proof
                .proof
                .iter()
                .rev()
                .map(|bytes| MptNode::decode(bytes).expect("Failed to decode storage proof"))
                .inspect(|node| drop(nodes_by_reference.insert(node.reference(), node.clone())))
                .last();
            // the hash of the root node should match the proof's storage hash
            assert_eq!(
                root_node.as_ref().map_or(EMPTY_ROOT, |n| n.hash()),
                from_ethers_h256(proof.storage_hash)
            );
        }

        let root_node = if let Some(root_node) = root_node {
            root_node
        } else if proof.storage_hash.0 == EMPTY_ROOT.0 {
            MptNode::default()
        } else {
            // if there are no storage proofs but the root is non-empty, create a dummy
            // as this is just the digest any tries to update this trie will fail
            MptNodeData::Digest(from_ethers_h256(proof.storage_hash)).into()
        };
        // collect all storage slots with a proof
        let slots = proof
            .storage_proof
            .into_iter()
            .map(|p| zeth_primitives::U256::from_be_bytes(p.key.into()))
            .collect();

        storage.insert(from_ethers_h160(proof.address), (root_node, slots));
    }
    (nodes_by_reference, storage)
}

fn resolve_orphans(
    nodes: &Vec<EthersBytes>,
    orphans: &mut HashSet<MptNodeReference>,
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) {
    for node in nodes {
        let mpt_node = MptNode::decode(node).expect("Failed to decode state proof");
        for potential_orphan in shorten_key(mpt_node) {
            let potential_orphan_hash = potential_orphan.reference();
            if orphans.remove(&potential_orphan_hash) {
                nodes_by_reference.insert(potential_orphan_hash, potential_orphan);
            }
        }
    }
}
