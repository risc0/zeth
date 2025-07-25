// Copyright 2025 RISC Zero, Inc.
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

use crate::rpc::DebugApi;
use alloy::{
    network::Network,
    primitives::{Address, B256, keccak256, map::B256Set},
    providers::Provider,
};
use anyhow::{Context, Result, bail};
use revm::database::StorageWithOriginalValues;
use risc0_ethereum_trie::{Nibbles, Trie, orphan};
use std::collections::HashSet;
use tracing::{debug, trace};

pub(crate) async fn handle_removed_account<P, N>(
    provider: &P,
    block_hash: B256,
    address: Address,
    state_trie: &mut Trie,
) -> Result<()>
where
    P: Provider<N>,
    N: Network,
{
    trace!(%address, "Hydrating proof for destroyed account");
    let proof = provider
        .get_proof(address, vec![])
        .hash(block_hash)
        .await
        .context("eth_getProof failed")?;
    state_trie.hydrate_from_rlp(&proof.account_proof)?;
    state_trie.resolve_orphan(keccak256(address), &proof.account_proof)?;

    Ok(())
}

pub(crate) async fn handle_new_account<P, N>(
    provider: &P,
    block_hash: B256,
    address: Address,
    state_trie: &mut Trie,
) -> Result<()>
where
    P: Provider<N>,
    N: Network,
{
    trace!(%address, "Hydrating proof for new account");
    let proof = provider
        .get_proof(address, vec![])
        .hash(block_hash)
        .await
        .context("eth_getProof failed")?;
    state_trie.hydrate_from_rlp(proof.account_proof)?;

    Ok(())
}

pub(crate) async fn handle_modified_account<P, N>(
    provider: &P,
    block_hash: B256,
    address: Address,
    storage: &StorageWithOriginalValues,
    storage_trie: &mut Trie,
) -> Result<()>
where
    P: Provider<N>,
    N: Network,
{
    // collect the storage keys for any new or removed slot
    let keys: Vec<B256> = storage
        .iter()
        .filter_map(|(key, slot)| {
            if slot.original_value().is_zero() != slot.present_value().is_zero() {
                Some(B256::from(*key))
            } else {
                None
            }
        })
        .collect();

    if keys.is_empty() {
        return Ok(());
    }

    trace!(%address, num_keys = keys.len(), "Hydrating proof for new or removed slots");
    let proof =
        provider.get_proof(address, keys).hash(block_hash).await.context("eth_getProof failed")?;

    let mut unresolvable: HashSet<Nibbles> = HashSet::default();
    for storage_proof in proof.storage_proof {
        let hashed_key = keccak256(storage_proof.key.as_b256());
        storage_trie.hydrate_from_rlp(&storage_proof.proof)?;
        if storage_proof.value.is_zero() {
            match storage_trie.resolve_orphan(hashed_key, &storage_proof.proof) {
                Ok(_) => {}
                Err(orphan::Error::Unresolvable(prefix)) => {
                    unresolvable.insert(prefix);
                }
                Err(err) => bail!(err),
            }
        }
    }

    if unresolvable.is_empty() {
        return Ok(());
    }

    debug!(%address, "Using debug_storageRangeAt to find preimages for orphan nodes");

    let mut missing_storage_keys = B256Set::default();
    for prefix in unresolvable {
        let storage_key = provider.get_next_storage_key(block_hash, address, prefix).await?;
        missing_storage_keys.insert(storage_key);
    }

    if !missing_storage_keys.is_empty() {
        trace!(%address, keys=?missing_storage_keys, "Fetching final proofs for missing storage keys");
        let proof = provider
            .get_proof(address, missing_storage_keys.into_iter().collect())
            .hash(block_hash)
            .await
            .context("eth_getProof failed")?;

        storage_trie.hydrate_from_rlp(proof.storage_proof.iter().flat_map(|p| &p.proof))?;
    }

    Ok(())
}
