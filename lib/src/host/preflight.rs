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

use std::fmt::Debug;

use anyhow::{anyhow, bail, ensure, Context, Ok, Result};
use ethers_core::types::{
    Block as EthersBlock, EIP1186ProofResponse, Transaction as EthersTransaction,
};
use hashbrown::HashMap;
use log::info;
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    keccak::keccak,
    transactions::{Transaction, TxEssence},
    trie::{MptNode, MptNodeData, MptNodeReference, EMPTY_ROOT},
    withdrawal::Withdrawal,
    Address, B256, U256,
};

use crate::{
    builder::{BlockBuilder, BlockBuilderStrategy},
    consts::ChainSpec,
    host::{
        mpt::{resolve_digests, shorten_key},
        provider::{new_provider, BlockQuery},
        provider_db::ProviderDb,
    },
    input::{Input, StorageEntry},
    mem_db::MemDb,
};

/// The initial data required to build a block as returned by the [Preflight].
#[derive(Debug, Clone)]
pub struct Data<E: TxEssence> {
    pub db: MemDb,
    pub parent_header: Header,
    pub parent_proofs: HashMap<Address, EIP1186ProofResponse>,
    pub header: Header,
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
        let parent_header: Header = parent_block.try_into().context("invalid parent block")?;

        // Fetch the target block
        let block = provider.get_full_block(&BlockQuery { block_no })?;

        info!(
            "Final block number: {:?} ({:?})",
            block.number.unwrap(),
            block.hash.unwrap()
        );
        info!("Transaction count: {:?}", block.transactions.len());

        // Create the provider DB
        let provider_db = ProviderDb::new(provider, parent_header.number);

        // Create the input data
        let input = new_preflight_input(block.clone(), parent_header.clone())?;
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
            parent_header,
            parent_proofs,
            header: block.try_into().context("invalid block")?,
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
        parent_header,
        ancestor_headers: Default::default(),
    };
    Ok(input)
}

/// Converts the [Data] returned by the [Preflight] into the [Input] required by the
/// [BlockBuilder].
impl<E: TxEssence> TryFrom<Data<E>> for Input<E> {
    type Error = anyhow::Error;

    fn try_from(data: Data<E>) -> Result<Input<E>> {
        // collect the code from each account
        let mut contracts = HashMap::new();
        for account in data.db.accounts.values() {
            let code = account.info.code.clone().unwrap();
            if !code.is_empty() {
                contracts.insert(code.hash_slow(), code.bytecode);
            }
        }

        let (state_trie, storage) = bar(
            data.parent_header.state_root,
            data.parent_proofs,
            data.proofs,
        )?;

        info!(
            "The partial state trie consists of {} nodes",
            state_trie.size()
        );
        info!(
            "The partial storage tries consist of {} nodes",
            storage.values().map(|(n, _)| n.size()).sum::<usize>()
        );

        // Create the block builder input
        let input = Input {
            parent_header: data.parent_header,
            beneficiary: data.header.beneficiary,
            gas_limit: data.header.gas_limit,
            timestamp: data.header.timestamp,
            extra_data: data.header.extra_data.0.clone().into(),
            mix_hash: data.header.mix_hash,
            transactions: data.transactions,
            withdrawals: data.withdrawals,
            parent_state_trie: state_trie,
            parent_storage: storage,
            contracts: contracts.into_values().collect(),
            ancestor_headers: data.ancestor_headers,
        };
        Ok(input)
    }
}

fn bar(
    state_root: B256,
    parent_proofs: HashMap<Address, EIP1186ProofResponse>,
    proofs: HashMap<Address, EIP1186ProofResponse>,
) -> Result<(MptNode, HashMap<Address, StorageEntry>)> {
    let mut storage: HashMap<Address, StorageEntry> = HashMap::with_capacity(parent_proofs.len());

    if parent_proofs.is_empty() {
        return Ok((MptNode::default(), storage));
    }

    let mut nodes_by_reference = HashMap::new();
    let empty_node = MptNode::default();
    nodes_by_reference.insert(empty_node.reference(), empty_node);

    let mut state_root_node = MptNode::default();
    for (address, proof) in parent_proofs {
        let proof_nodes = parse_proof(&proof.account_proof)?;
        let proof_trie = from_proof(&proof_nodes)?;
        ensure!(proof_trie.hash() == state_root, "state root mismatch");

        if let Some(n) = proof_nodes.first() {
            state_root_node = n.clone();
        }

        proof_nodes.into_iter().for_each(|node| {
            nodes_by_reference.insert(node.reference(), node);
        });

        let fini_proofs = proofs.get(&address).unwrap();

        add_deleted_nodes(address, &fini_proofs.account_proof, &mut nodes_by_reference)?;

        let storage_root = from_ethers_h256(proof.storage_hash);

        if proof.storage_proof.is_empty() {
            let storage_root_node = node_from_digest(storage_root);
            storage.insert(address, (storage_root_node, vec![]));
            continue;
        }

        let mut storage_root_node = MptNode::default();
        for storage_proof in &proof.storage_proof {
            let proof_nodes = parse_proof(&storage_proof.proof)?;
            let proof_trie = from_proof(&proof_nodes)?;
            ensure!(proof_trie.hash() == storage_root, "storage root mismatch");

            if let Some(n) = proof_nodes.first() {
                storage_root_node = n.clone();
            }

            proof_nodes.into_iter().for_each(|node| {
                nodes_by_reference.insert(node.reference(), node);
            });
        }

        for storage_proof in &fini_proofs.storage_proof {
            add_deleted_nodes(
                storage_proof.key,
                &storage_proof.proof,
                &mut nodes_by_reference,
            )?;
        }
        let storage_trie = resolve_digests(&storage_root_node, &nodes_by_reference);
        assert_eq!(storage_trie.hash(), storage_root);

        let slots = proof
            .storage_proof
            .iter()
            .map(|p| U256::from_be_bytes(p.key.into()))
            .collect();
        storage.insert(address, (storage_trie, slots));
    }
    let state_trie = resolve_digests(&state_root_node, &nodes_by_reference);
    assert_eq!(state_trie.hash(), state_root);

    Ok((state_trie, storage))
}

fn node_from_digest(digest: B256) -> MptNode {
    match digest {
        EMPTY_ROOT | B256::ZERO => MptNode::default(),
        _ => MptNodeData::Digest(digest).into(),
    }
}

fn add_deleted_nodes(
    key: impl AsRef<[u8]>,
    proof: &Vec<impl AsRef<[u8]>>,
    nodes_by_reference: &mut HashMap<MptNodeReference, MptNode>,
) -> Result<()> {
    if !proof.is_empty() {
        let proof_nodes = parse_proof(proof)?;
        let proof_trie = from_proof(&proof_nodes)?;
        let value = proof_trie.get(&keccak(key))?;
        if value.is_none() {
            let leaf = proof_nodes.last().unwrap();

            shorten_key(leaf).into_iter().for_each(|node| {
                nodes_by_reference.insert(node.reference(), node);
            });
        }
    }

    Ok(())
}

fn parse_proof(proof: &[impl AsRef<[u8]>]) -> Result<Vec<MptNode>> {
    Ok(proof
        .iter()
        .map(MptNode::decode)
        .collect::<Result<Vec<_>, _>>()?)
}

/// Creates a Merkle Patricia tree from an EIP-1186 proof.
pub fn from_proof(proof_nodes: &[MptNode]) -> Result<MptNode> {
    let mut next: Option<MptNode> = None;
    for (i, node) in proof_nodes.iter().enumerate().rev() {
        // there is nothing to resolve in the last node of the proof
        let Some(replacement) = next else {
            next = Some(node.clone());
            continue;
        };

        // otherwise, resolve the reference to the next node in the proof
        let MptNodeReference::Digest(to_resolve) = replacement.reference() else {
            bail!("node {} in proof is not referenced by hash", i + 1);
        };

        let resolved: MptNode = match node.as_data().clone() {
            MptNodeData::Null | MptNodeData::Leaf(_, _) | MptNodeData::Digest(_) => {
                bail!("node {} has no children to replace", i);
            }
            MptNodeData::Branch(mut children) => {
                if let Some(child) = children.iter_mut().flatten().find(|child|
                     matches!(child.as_data(), MptNodeData::Digest(digest) if digest == &to_resolve)) {
                *child = Box::new(replacement);
            } else {
                bail!("node {} does not reference the successor", i);
            }
                MptNodeData::Branch(children).into()
            }
            MptNodeData::Extension(prefix, child) => {
                if !matches!(child.as_data(), MptNodeData::Digest(dig) if dig == &to_resolve) {
                    bail!("node {} does not reference the successor", i);
                }
                MptNodeData::Extension(prefix, Box::new(replacement)).into()
            }
        };

        next = Some(resolved);
    }

    Ok(next.unwrap_or_default())
}
