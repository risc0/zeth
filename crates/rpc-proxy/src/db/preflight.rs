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

use crate::db::{ProviderDb, provider};
use alloy::{
    consensus::BlockHeader,
    eips::eip2930::{AccessList, AccessListItem},
    network::{BlockResponse, Network},
    providers::Provider,
    rlp::decode_exact,
    rpc::types::EIP1186AccountProofResponse,
};
use alloy_primitives::{
    Address, B256, BlockNumber, Bytes, KECCAK256_EMPTY, StorageKey, StorageValue, U256, keccak256,
    map::{
        AddressHashMap, AddressMap, B256HashMap, B256HashSet, B256Map, HashMap, HashSet, hash_map,
    },
};
use alloy_trie::{EMPTY_ROOT_HASH, TrieAccount as StateAccount};
use anyhow::{Context, Result, ensure};
use itertools::Itertools;
use revm::{
    Database as RevmDatabase,
    context::DBErrorMarker,
    state::{AccountInfo, Bytecode},
};
use risc0_ethereum_trie::{Trie as MerkleTrie, Trie};
use std::{
    fmt::{self, Debug},
    hash::{BuildHasher, Hash},
};

/// A simple revm [RevmDatabase] wrapper that records all DB queries.
#[derive(Clone, Default)]
pub struct PreflightDb<D> {
    accounts: AddressHashMap<B256HashSet>,
    contracts: B256HashMap<Bytes>,
    block_hash_numbers: HashSet<BlockNumber>,

    code_addresses: B256Map<Address>,
    proofs: AccountProofs,
    inner: D,
}

#[derive(Clone, Default)]
struct AccountProofs(AddressHashMap<AccountProof>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountProof {
    /// The account information as stored in the account trie.
    account: Option<StateAccount>,
    /// The inclusion proof for this account.
    account_proof: Vec<Bytes>,
    /// The MPT inclusion proofs for several storage slots.
    storage_proofs: B256HashMap<StorageProof>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StorageProof {
    /// The value that this key holds.
    value: StorageValue,
    /// In MPT inclusion proof for this particular slot.
    proof: Vec<Bytes>,
}

impl<D> Debug for PreflightDb<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreflightDb")
            .field("accounts", &self.accounts)
            .field("contracts", &self.contracts)
            .field("block_hash_numbers", &self.block_hash_numbers)
            .finish()
    }
}

impl<D> PreflightDb<D> {
    /// Creates a new ProofDb instance, with a [RevmDatabase].
    pub(crate) fn new(db: D) -> Self
    where
        D: RevmDatabase,
    {
        Self {
            accounts: Default::default(),
            contracts: Default::default(),
            block_hash_numbers: Default::default(),
            code_addresses: Default::default(),
            proofs: Default::default(),
            inner: db,
        }
    }

    /// Returns the referenced contracts
    pub(crate) fn contracts(&self) -> &B256HashMap<Bytes> {
        &self.contracts
    }
}

impl<N: Network, P: Provider<N>> PreflightDb<ProviderDb<N, P>> {
    /// Fetches all the EIP-1186 storage proofs from the `access_list` and stores them in the DB.
    pub(crate) async fn add_access_list(&mut self, access_list: &AccessList) -> Result<()> {
        for AccessListItem { address, storage_keys } in &access_list.0 {
            if let Some(keys) = self.proofs.missing_proof(address, storage_keys) {
                let proof = self.inner.get_proof(*address, keys).await?;
                self.proofs.add(proof).context("invalid eth_getProof response")?;
            }
        }

        Ok(())
    }

    /// Returns the chain of ancestor headers starting from `start_hash`.
    ///
    /// This trace continues until it reaches a block number lower than the minimum
    /// number recorded in `self.block_hash_numbers`.
    pub(crate) async fn ancestor_proof(
        &self,
        start_hash: B256,
    ) -> Result<Vec<<N as Network>::HeaderResponse>> {
        let provider = self.inner.provider();
        let mut ancestors = Vec::new();
        let mut current_hash = start_hash;
        let mut min_number: Option<u64> = None;

        loop {
            let rpc_block = provider
                .get_block_by_hash(current_hash)
                .await
                .context("eth_getBlockByHash failed")?
                .with_context(|| format!("block {current_hash} not found"))?;
            let header = rpc_block.header().clone();

            // lazily determine the minimum block number on the first iteration
            let block_hash_min_number = *min_number.get_or_insert_with(|| {
                *self.block_hash_numbers.iter().min().unwrap_or(&header.number())
            });

            current_hash = header.parent_hash();
            let block_number = header.number();
            ancestors.push(header);

            if block_number <= block_hash_min_number {
                break;
            }
        }

        Ok(ancestors)
    }

    /// Returns the merkle proofs (sparse [MerkleTrie]) for the state and all storage queries
    /// recorded by the [RevmDatabase].
    pub(crate) async fn state_proof(&mut self) -> Result<(MerkleTrie, AddressMap<MerkleTrie>)> {
        // if no accounts were accessed, use the state root of the corresponding block as is
        if self.accounts.is_empty() {
            let hash = self.inner.block();
            let block = self
                .inner
                .provider()
                .get_block_by_hash(hash)
                .await
                .context("eth_getBlockByHash failed")?
                .with_context(|| format!("block {hash} not found"))?;

            return Ok((
                MerkleTrie::from_digest(block.header().state_root()),
                AddressMap::default(),
            ));
        }

        let proofs = &mut self.proofs;
        for (address, storage_keys) in &self.accounts {
            if let Some(keys) = proofs.missing_proof(address, storage_keys) {
                let proof = self.inner.get_proof(*address, keys).await?;
                proofs.add(proof).context("invalid eth_getProof response")?;
            }
        }

        let state_nodes = self
            .accounts
            .keys()
            .filter_map(|address| proofs.get(address))
            .flat_map(|proof| proof.account_proof.iter());
        let state_trie = MerkleTrie::from_rlp(state_nodes).context("accountProof invalid")?;

        let mut storage_tries: AddressMap<MerkleTrie> = AddressMap::default();
        for (address, storage_keys) in &self.accounts {
            // safe unwrap: added a proof for each account in the previous loop
            let proof = proofs.get(address).unwrap();

            // create a new trie for this root
            let storage_root = proof.account.map(|a| a.storage_root).unwrap_or(EMPTY_ROOT_HASH);
            let mut storage_trie = MerkleTrie::from_digest(storage_root);

            // hydrate the trie if storage slots were accessed
            if !storage_keys.is_empty() {
                let storage_nodes = storage_keys
                    .iter()
                    .filter_map(|key| proof.storage_proofs.get(key))
                    .flat_map(|proof| proof.proof.iter());

                storage_trie
                    .hydrate_from_rlp(storage_nodes)
                    .with_context(|| format!("invalid storage proof for address {address}"))?;
            }

            ensure!(storage_trie.hash_slow() == storage_root, "storage root mismatch");
            storage_tries.insert(*address, storage_trie);
        }

        Ok((state_trie, storage_tries))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("provider error")]
    Provider(#[from] provider::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl DBErrorMarker for DbError {}

impl<N: Network, P: Provider<N>> RevmDatabase for PreflightDb<ProviderDb<N, P>> {
    type Error = DbError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.accounts.entry(address).or_default();

        let account = match self.proofs.get(&address) {
            Some(proof) => proof.account,
            None => {
                let proof = self.inner.get_proof_blocking(address, vec![])?;
                self.proofs.add(proof).context("invalid proof response")?
            }
        };
        let code_hash = account.map(|acc| acc.code_hash).unwrap_or(KECCAK256_EMPTY);
        if code_hash != KECCAK256_EMPTY {
            self.code_addresses.insert(code_hash, address);
        }

        Ok(account.map(|acc| AccountInfo {
            balance: acc.balance,
            nonce: acc.nonce,
            code_hash: acc.code_hash,
            code: None, // will be queried later using code_by_hash
        }))
    }

    fn code_by_hash(&mut self, hash: B256) -> Result<Bytecode, Self::Error> {
        let code = match self.code_addresses.get(&hash) {
            None => self.inner.code_by_hash(hash)?,
            Some(address) => self.inner.get_code_at(*address)?,
        };
        self.contracts.insert(hash, code.original_bytes());

        Ok(code)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let key = StorageKey::from(index);
        self.accounts.entry(address).or_default().insert(key);

        // try to get the storage value from the loaded proofs before querying the underlying DB
        match self.proofs.get(&address).and_then(|account| account.storage_proofs.get(&key)) {
            Some(storage_proof) => Ok(storage_proof.value),
            None => Ok(self.inner.storage(address, index)?),
        }
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.block_hash_numbers.insert(number);

        Ok(self.inner.block_hash(number)?)
    }
}

impl AccountProofs {
    fn get(&self, address: &Address) -> Option<&AccountProof> {
        self.0.get(address)
    }

    fn add(&mut self, proof_response: EIP1186AccountProofResponse) -> Result<Option<StateAccount>> {
        // extract the actual state account from the proof
        let account = decode_account(&proof_response).context("invalid account proof")?;

        // convert the response into a StorageProof
        let storage_proofs = proof_response
            .storage_proof
            .into_iter()
            .map(|proof| {
                (proof.key.as_b256(), StorageProof { value: proof.value, proof: proof.proof })
            })
            .collect();

        match self.0.entry(proof_response.address) {
            hash_map::Entry::Occupied(mut entry) => {
                let account_proof = entry.get_mut();
                ensure!(
                    account_proof.account == account
                        && account_proof.account_proof == proof_response.account_proof,
                    "inconsistent account proof"
                );
                account_proof.storage_proofs = merge_checked_maps(
                    std::mem::take(&mut account_proof.storage_proofs),
                    storage_proofs,
                );
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(AccountProof {
                    account,
                    account_proof: proof_response.account_proof,
                    storage_proofs,
                });
            }
        }

        Ok(account)
    }

    fn missing_proof<'a>(
        &self,
        address: &Address,
        keys: impl IntoIterator<Item = &'a StorageKey>,
    ) -> Option<Vec<StorageKey>> {
        let Some(proof) = self.get(address) else {
            return Some(keys.into_iter().cloned().unique().collect());
        };

        let storage_root = proof.account.map_or(EMPTY_ROOT_HASH, |a| a.storage_root);
        if storage_root == EMPTY_ROOT_HASH {
            return None;
        }

        let new_key = |k: &&StorageKey| !proof.storage_proofs.contains_key(*k);
        let missing_keys: Vec<_> = keys.into_iter().filter(new_key).cloned().unique().collect();

        // we only need to request additional proofs if some keys are missing
        if missing_keys.is_empty() { None } else { Some(missing_keys) }
    }
}

/// Merges two HashMaps, checking for consistency on overlapping keys.
/// Panics if values for the same key are different. Consumes both maps.
fn merge_checked_maps<K, V, S, T>(mut map: HashMap<K, V, S>, iter: T) -> HashMap<K, V, S>
where
    K: Eq + Hash + Debug,
    V: PartialEq + Debug,
    S: BuildHasher,
    T: IntoIterator<Item = (K, V)>,
{
    let iter = iter.into_iter();
    let (lower_bound, _) = iter.size_hint();
    map.reserve(lower_bound);

    for (key, value2) in iter {
        match map.entry(key) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(value2);
            }
            hash_map::Entry::Occupied(entry) => {
                let value1 = entry.get();
                if value1 != &value2 {
                    panic!(
                        "mismatching values for key {:?}: existing={:?}, other={:?}",
                        entry.key(),
                        value1,
                        value2
                    );
                }
            }
        }
    }

    map
}

fn decode_account(proof_response: &EIP1186AccountProofResponse) -> Result<Option<StateAccount>> {
    let trie = Trie::from_rlp(&proof_response.account_proof)?;
    match trie.get(keccak256(proof_response.address)) {
        None => Ok(None),
        Some(rlp) => Ok(Some(decode_exact(rlp)?)),
    }
}
