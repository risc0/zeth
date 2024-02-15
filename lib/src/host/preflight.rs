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

use std::{
    fmt::Debug,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context, Result};
use ethers_core::types::{
    Block as EthersBlock, EIP1186ProofResponse, Transaction as EthersTransaction,
};
use hashbrown::{HashMap, HashSet};
use log::{debug, info};
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    keccak::keccak,
    transactions::{Transaction, TxEssence},
    trie::{MptNode, MptNodeReference},
    withdrawal::Withdrawal,
    Address, B256, U256,
};

use crate::{
    builder::{BlockBuilder, BlockBuilderStrategy},
    consts::ChainSpec,
    host::{
        mpt::{is_not_included, mpt_from_proof, parse_proof, resolve_nodes, shorten_node_path},
        provider::{new_provider, BlockQuery},
        provider_db::ProviderDb,
    },
    input::{BlockBuildInput, StateInput, StorageEntry},
    mem_db::MemDb,
};

/// The initial data required to build a block as returned by the [Preflight].
#[derive(Debug, Clone)]
pub struct Data<E: TxEssence> {
    pub db: MemDb,
    pub parent_header: Header,
    pub parent_proofs: HashMap<Address, EIP1186ProofResponse>,
    pub header: Option<Header>,
    pub transactions: Vec<Transaction<E>>,
    pub withdrawals: Vec<Withdrawal>,
    pub proofs: HashMap<Address, EIP1186ProofResponse>,
    pub ancestor_headers: Vec<Header>,
}

pub trait Preflight<E: TxEssence> {
    /// Executes the complete block using the input and state from the RPC provider.
    /// It returns all the data required to build and validate the block.
    fn preflight_with_external_data(
        chain_spec: &ChainSpec,
        cache_path: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> Result<Data<E>>;

    fn preflight_with_local_data(
        chain_spec: &ChainSpec,
        provider_db: ProviderDb,
        input: BlockBuildInput<E>,
    ) -> Result<Data<E>>;
}

/// Implements the [Preflight] trait for all compatible [BlockBuilderStrategy]s.
impl<N: BlockBuilderStrategy> Preflight<N::TxEssence> for N
where
    N::TxEssence: TryFrom<EthersTransaction>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
{
    fn preflight_with_external_data(
        chain_spec: &ChainSpec,
        cache_path: Option<PathBuf>,
        rpc_url: Option<String>,
        block_no: u64,
    ) -> Result<Data<N::TxEssence>> {
        let mut provider = new_provider(cache_path, rpc_url)?;

        // Fetch the parent block
        let parent_block = provider.get_partial_block(&BlockQuery {
            block_no: block_no - 1,
        })?;

        debug!(
            "Initial block: {:?} ({:?})",
            parent_block.number.unwrap(),
            parent_block.hash.unwrap()
        );
        let parent_header: Header = parent_block.try_into().context("invalid parent block")?;

        // Fetch the target block
        let block = provider.get_full_block(&BlockQuery { block_no })?;

        debug!(
            "Final block number: {:?} ({:?})",
            block.number.unwrap(),
            block.hash.unwrap()
        );
        debug!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = ProviderDb::new(provider, parent_header.number);

        // Create the input data
        let input = new_preflight_input(block.clone(), parent_header.clone())?;

        // Create the block builder, run the transactions and extract the DB
        Self::preflight_with_local_data(chain_spec, provider_db, input).map(
            move |mut headerless_preflight_data| {
                headerless_preflight_data.header = Some(block.try_into().expect("invalid block"));
                headerless_preflight_data
            },
        )
    }

    fn preflight_with_local_data(
        chain_spec: &ChainSpec,
        provider_db: ProviderDb,
        input: BlockBuildInput<N::TxEssence>,
    ) -> Result<Data<N::TxEssence>> {
        let parent_header = input.state_input.parent_header.clone();
        let transactions = input.state_input.transactions.clone();
        let withdrawals = input.state_input.withdrawals.clone();
        // Create the block builder, run the transactions and extract the DB even if run fails
        let db_backup = Arc::new(Mutex::new(None));
        let builder =
            BlockBuilder::new(chain_spec, input, Some(db_backup.clone())).with_db(provider_db);
        let mut provider_db = match builder.prepare_header::<N::HeaderPrepStrategy>() {
            Ok(builder) => match builder.execute_transactions::<N::TxExecStrategy>() {
                Ok(builder) => builder.take_db().unwrap(),
                Err(_) => db_backup.lock().unwrap().take().unwrap(),
            },
            Err(_) => db_backup.lock().unwrap().take().unwrap(),
        };

        info!("Gathering inclusion proofs ...");

        // Gather inclusion proofs for the initial and final state
        let parent_proofs = provider_db.get_initial_proofs()?;
        let proofs = provider_db.get_latest_proofs()?;

        // Gather proofs for block history
        let ancestor_headers = provider_db.get_ancestor_headers()?;

        info!("Saving provider cache ...");

        // Save the provider cache
        provider_db.save_provider()?;

        info!("Provider-backed execution is Done!");

        // Fetch the target block
        Ok(Data {
            db: provider_db.get_initial_db().clone(),
            parent_header,
            parent_proofs,
            header: None,
            transactions,
            withdrawals,
            proofs,
            ancestor_headers,
        })
    }
}

fn new_preflight_input<E>(
    block: EthersBlock<EthersTransaction>,
    parent_header: Header,
) -> Result<BlockBuildInput<E>>
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

    let verifying_key_hints: Vec<_> = transactions
        .iter()
        .map(|tx: &Transaction<E>| tx.verifying_key().ok())
        .collect();

    let input = BlockBuildInput {
        state_input: StateInput {
            parent_header,
            beneficiary: from_ethers_h160(block.author.context("author missing")?),
            gas_limit: from_ethers_u256(block.gas_limit),
            timestamp: from_ethers_u256(block.timestamp),
            extra_data: block.extra_data.0.into(),
            mix_hash: from_ethers_h256(block.mix_hash.context("mix_hash missing")?),
            transactions,
            withdrawals,
        },
        verifying_key_hints,
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        ancestor_headers: Default::default(),
    };
    Ok(input)
}

/// Converts the [Data] returned by the [Preflight] into
/// [BlockBuildInput] required by the [BlockBuilder].
impl<E: TxEssence> TryFrom<Data<E>> for BlockBuildInput<E> {
    type Error = anyhow::Error;

    fn try_from(data: Data<E>) -> Result<BlockBuildInput<E>> {
        // collect the code from each account
        let mut contracts = HashSet::new();
        for account in data.db.accounts.values() {
            let code = account.info.code.clone().context("missing code")?;
            if !code.is_empty() {
                contracts.insert(code.bytecode);
            }
        }

        // construct the sparse MPTs from the inclusion proofs
        let (state_trie, storage) = proofs_to_tries(
            data.parent_header.state_root,
            data.parent_proofs,
            data.proofs,
        )?;

        debug!(
            "The partial state trie consists of {} nodes",
            state_trie.size()
        );
        debug!(
            "The partial storage tries consist of {} nodes",
            storage.values().map(|(n, _)| n.size()).sum::<usize>()
        );

        let verifying_key_hints: Vec<_> = data
            .transactions
            .iter()
            .map(|tx| tx.verifying_key().ok())
            .collect();

        // Create the block builder input
        let header = data.header.as_ref().expect("Missing header data");
        let input = BlockBuildInput {
            state_input: StateInput {
                parent_header: data.parent_header,
                beneficiary: header.beneficiary,
                gas_limit: header.gas_limit,
                timestamp: header.timestamp,
                extra_data: header.extra_data.0.clone().into(),
                mix_hash: header.mix_hash,
                transactions: data.transactions,
                withdrawals: data.withdrawals,
            },
            verifying_key_hints,
            parent_state_trie: state_trie,
            parent_storage: storage,
            contracts: contracts.into_iter().collect(),
            ancestor_headers: data.ancestor_headers,
        };
        Ok(input)
    }
}

fn proofs_to_tries(
    state_root: B256,
    parent_proofs: HashMap<Address, EIP1186ProofResponse>,
    proofs: HashMap<Address, EIP1186ProofResponse>,
) -> Result<(MptNode, HashMap<Address, StorageEntry>)> {
    // if no addresses are provided, return the trie only consisting of the state root
    if parent_proofs.is_empty() {
        return Ok((state_root.into(), HashMap::new()));
    }

    let mut storage: HashMap<Address, StorageEntry> = HashMap::with_capacity(parent_proofs.len());

    let mut state_nodes = HashMap::new();
    let mut state_root_node = MptNode::default();
    for (address, proof) in parent_proofs {
        let proof_nodes =
            parse_proof(&proof.account_proof).context("invalid account_proof encoding")?;
        mpt_from_proof(&proof_nodes).context("invalid account_proof")?;

        // the first node in the proof is the root
        if let Some(node) = proof_nodes.first() {
            state_root_node = node.clone();
        }

        proof_nodes.into_iter().for_each(|node| {
            state_nodes.insert(node.reference(), node);
        });

        let fini_proofs = proofs
            .get(&address)
            .with_context(|| format!("missing fini_proofs for address {:#}", &address))?;

        // assure that addresses can be deleted from the state trie
        add_orphaned_leafs(address, &fini_proofs.account_proof, &mut state_nodes)?;

        // if no slots are provided, return the trie only consisting of the storage root
        let storage_root = from_ethers_h256(proof.storage_hash);
        if proof.storage_proof.is_empty() {
            storage.insert(address, (storage_root.into(), vec![]));
            continue;
        }

        let mut storage_nodes = HashMap::new();
        let mut storage_root_node = MptNode::default();
        for storage_proof in &proof.storage_proof {
            let proof_nodes =
                parse_proof(&storage_proof.proof).context("invalid storage_proof encoding")?;
            mpt_from_proof(&proof_nodes).context("invalid storage_proof")?;

            // the first node in the proof is the root
            if let Some(node) = proof_nodes.first() {
                storage_root_node = node.clone();
            }

            proof_nodes.into_iter().for_each(|node| {
                storage_nodes.insert(node.reference(), node);
            });
        }

        // assure that slots can be deleted from the storage trie
        for storage_proof in &fini_proofs.storage_proof {
            add_orphaned_leafs(storage_proof.key, &storage_proof.proof, &mut storage_nodes)?;
        }
        // create the storage trie, from all the relevant nodes
        let storage_trie = resolve_nodes(&storage_root_node, &storage_nodes);
        assert_eq!(storage_trie.hash(), storage_root);

        // convert the slots to a vector of U256
        let slots = proof
            .storage_proof
            .iter()
            .map(|p| U256::from_be_bytes(p.key.into()))
            .collect();
        storage.insert(address, (storage_trie, slots));
    }
    let state_trie = resolve_nodes(&state_root_node, &state_nodes);
    assert_eq!(state_trie.hash(), state_root);

    Ok((state_trie, storage))
}

/// Adds all the leaf nodes of non-inclusion proofs to the nodes.
fn add_orphaned_leafs(
    key: impl AsRef<[u8]>,
    proof: &[impl AsRef<[u8]>],
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) -> Result<()> {
    if !proof.is_empty() {
        let proof_nodes = parse_proof(proof).context("invalid proof encoding")?;
        if is_not_included(&keccak(key), &proof_nodes)? {
            // add the leaf node to the nodes
            let leaf = proof_nodes.last().unwrap();
            shorten_node_path(leaf).into_iter().for_each(|node| {
                nodes_by_reference.insert(node.reference(), node);
            });
        }
    }

    Ok(())
}
