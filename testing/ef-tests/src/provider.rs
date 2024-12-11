use crate::{TestBlock, TestState};
use alloy::{
    consensus::Account,
    network::{Ethereum, Network},
    primitives::{keccak256, Address, Bytes, B256, U256},
    rpc::types::{
        Block, BlockTransactions, EIP1186AccountProofResponse, EIP1186StorageProof, Header,
    },
};
use alloy_trie::proof::ProofRetainer;
use anyhow::anyhow;
use nybbles::Nibbles;
use reth_chainspec::NamedChain;
use std::{
    collections::{
        BTreeMap,
        Bound::{Excluded, Unbounded},
    },
    iter, vec,
};
use zeth_preflight::provider::{
    query::{
        AccountQuery, AccountRangeQuery, BlockQuery, PreimageQuery, ProofQuery, StorageQuery,
        StorageRangeQuery, UncleQuery,
    },
    Provider,
};

/// Provider that always returns the default if not contained in the state.
pub struct TestProvider {
    genesis: Header,
    block: TestBlock,
    pre: ProviderState,
    post: ProviderState,
}

struct ProviderState(BTreeMap<B256, ProviderAccount>);

struct ProviderAccount {
    address: Address,
    storage: BTreeMap<B256, (B256, U256)>,
    code: Bytes,
    acc: Account,
}

impl TestProvider {
    pub fn new(genesis: Header, block: TestBlock, pre: TestState, post: TestState) -> Self {
        TestProvider {
            genesis,
            block,
            pre: pre.into(),
            post: post.into(),
        }
    }
}

impl Provider<Ethereum> for TestProvider {
    fn save(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn advance(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self, _block_number: u64) -> anyhow::Result<()> {
        Ok(())
    }

    fn get_client_version(&mut self) -> anyhow::Result<String> {
        unimplemented!("get_client_version")
    }

    fn get_chain(&mut self) -> anyhow::Result<NamedChain> {
        Ok(NamedChain::Mainnet)
    }

    fn get_full_block(
        &mut self,
        query: &BlockQuery,
    ) -> anyhow::Result<<Ethereum as Network>::BlockResponse> {
        if query.block_no == 0 {
            return Ok(Block {
                header: self.genesis.clone(),
                uncles: vec![],
                transactions: BlockTransactions::Full(vec![]),
                size: None,
                withdrawals: None,
            });
        }

        assert_eq!(query.block_no, 1);
        let block = self.block.clone();
        Ok(Block {
            header: block.block_header.unwrap(),
            uncles: block.uncle_headers.iter().map(|h| h.hash).collect(),
            transactions: BlockTransactions::Full(block.transactions),
            size: None,
            withdrawals: block.withdrawals,
        })
    }

    fn get_uncle_block(
        &mut self,
        query: &UncleQuery,
    ) -> anyhow::Result<<Ethereum as Network>::BlockResponse> {
        assert_eq!(query.block_no, 1);
        let uncle = self.block.uncle_headers[query.uncle_index as usize].clone();
        Ok(Block {
            header: uncle,
            uncles: vec![],
            transactions: BlockTransactions::Uncle,
            size: None,
            withdrawals: None,
        })
    }

    fn get_block_receipts(
        &mut self,
        _: &BlockQuery,
    ) -> anyhow::Result<Vec<<Ethereum as Network>::ReceiptResponse>> {
        unimplemented!("get_block_receipts")
    }

    fn get_proof(&mut self, query: &ProofQuery) -> anyhow::Result<EIP1186AccountProofResponse> {
        match query.block_no {
            0 => Ok(self.pre.get_proof(query.address, &query.indices)),
            1 => Ok(self.post.get_proof(query.address, &query.indices)),
            block_no => Err(anyhow!("no state for block: {block_no}")),
        }
    }

    fn get_transaction_count(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        assert_eq!(query.block_no, 0);
        let nonce = self.pre.get_nonce(&query.address).unwrap_or_default();
        Ok(U256::from(nonce))
    }

    fn get_balance(&mut self, query: &AccountQuery) -> anyhow::Result<U256> {
        assert_eq!(query.block_no, 0);
        let balance = self.pre.get_balance(&query.address);
        Ok(balance.unwrap_or_default())
    }

    fn get_code(&mut self, query: &AccountQuery) -> anyhow::Result<Bytes> {
        assert_eq!(query.block_no, 0);
        let code = self.pre.get_code(&query.address);
        Ok(code.unwrap_or_default())
    }

    fn get_storage(&mut self, query: &StorageQuery) -> anyhow::Result<U256> {
        assert_eq!(query.block_no, 0);
        let value = self.pre.get_storage(&query.address, query.index);
        Ok(value.unwrap_or_default())
    }

    fn get_preimage(&mut self, _query: &PreimageQuery) -> anyhow::Result<Bytes> {
        unimplemented!("get_preimage")
    }

    fn get_next_account(&mut self, query: &AccountRangeQuery) -> anyhow::Result<Address> {
        assert_eq!(query.block_no, 0);
        self.pre
            .get_next_account(query.start)
            .ok_or(anyhow!("no next account"))
    }

    fn get_next_slot(&mut self, query: &StorageRangeQuery) -> anyhow::Result<U256> {
        assert_eq!(query.block_no, 0);
        let next = self
            .pre
            .get_next_slot(query.address, query.start)
            .ok_or(anyhow!("no next slot"))?;
        Ok(next.into())
    }
}

impl ProviderState {
    fn get_proof<'a>(
        &self,
        address: Address,
        indices: impl IntoIterator<Item = &'a B256>,
    ) -> EIP1186AccountProofResponse {
        let key = keccak256(&address);
        let account = self.0.get(&key);

        let mut storage_proof = Vec::new();
        let storage = account.map(|a| &a.storage).cloned().unwrap_or_default();
        for index in indices {
            let key = keccak256(index);
            let (_, proof) = mpt_proof(storage.iter().map(|(k, v)| (k, v.1)), iter::once(&key));
            storage_proof.push(EIP1186StorageProof {
                key: (*index).into(),
                value: storage.get(&key).map(|v| v.1).unwrap_or_default(),
                proof,
            })
        }

        let account = account.map(|a| a.acc.clone()).unwrap_or_default();
        let (_, account_proof) = mpt_proof(
            self.0.iter().map(|(addr, acc)| (addr, acc.acc)),
            iter::once(&key),
        );

        EIP1186AccountProofResponse {
            address,
            balance: account.balance,
            code_hash: account.code_hash,
            nonce: account.nonce,
            storage_hash: account.storage_root,
            account_proof,
            storage_proof,
        }
    }

    fn get_nonce(&self, address: &Address) -> Option<u64> {
        self.0.get(&keccak256(address)).map(|a| a.acc.nonce)
    }

    fn get_balance(&self, address: &Address) -> Option<U256> {
        self.0.get(&keccak256(address)).map(|a| a.acc.balance)
    }

    fn get_code(&self, address: &Address) -> Option<Bytes> {
        self.0.get(&keccak256(address)).map(|a| a.code.clone())
    }

    fn get_storage(&self, address: &Address, index: impl Into<B256>) -> Option<U256> {
        let account = self.0.get(&keccak256(address));
        let key = keccak256(index.into());
        account.and_then(|a| a.storage.get(&key)).map(|v| v.1)
    }

    fn get_next_account(&self, start: B256) -> Option<Address> {
        let next = self.0.range((Excluded(start), Unbounded)).next();
        next.map(|(_, v)| v.address)
    }

    fn get_next_slot(&self, address: Address, start: B256) -> Option<B256> {
        let Some(account) = self.0.get(&keccak256(address)) else {
            return None;
        };
        let next = account.storage.range((Excluded(start), Unbounded)).next();
        next.map(|(_, v)| v.0)
    }
}

impl From<TestState> for ProviderState {
    fn from(test_state: TestState) -> Self {
        let mut state = BTreeMap::new();
        for (address, test_account) in test_state.0 {
            let key = keccak256(address);
            let storage: BTreeMap<B256, (B256, U256)> = test_account
                .storage
                .into_iter()
                .map(|(k, v)| (keccak256(B256::from(k)), (B256::from(k), v)))
                .collect();
            let (storage_root, _) = mpt_proof(storage.iter().map(|(k, v)| (k, v.1)), iter::empty());
            let code = test_account.code;
            let acc = Account {
                nonce: test_account.nonce,
                balance: test_account.balance,
                storage_root,
                code_hash: keccak256(&code),
            };

            state.insert(
                key,
                ProviderAccount {
                    address,
                    storage,
                    code,
                    acc,
                },
            );
        }

        ProviderState(state)
    }
}

fn mpt_proof<K: AsRef<[u8]>, V: alloy::rlp::Encodable>(
    leaves: impl IntoIterator<Item = (K, V)>,
    targets: impl IntoIterator<Item = K>,
) -> (B256, Vec<Bytes>) {
    let mut hasher = alloy_trie::HashBuilder::default();
    let targets: Vec<_> = targets.into_iter().map(|t| Nibbles::unpack(t)).collect();
    if targets.len() > 0 {
        hasher = hasher.with_proof_retainer(ProofRetainer::new(targets));
    }
    for (key, value) in leaves {
        hasher.add_leaf(Nibbles::unpack(key), alloy::rlp::encode(value).as_slice())
    }
    let root = hasher.root();
    let proof = hasher
        .take_proof_nodes()
        .into_nodes_sorted()
        .into_iter()
        .map(|(_, b)| b)
        .collect();

    (root, proof)
}
