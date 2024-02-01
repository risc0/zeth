use std::sync::{Arc, Mutex};

use lru_time_cache::LruCache;
use zeth_primitives::{Address, B256};

use super::ProofType;
pub struct CachedProof {
    proof: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct CacheKey {
    pub proof_type: ProofType,
    pub block: u64,
    pub prover: Address,
    pub graffiti: B256,
}

impl AsRef<CacheKey> for CacheKey {
    fn as_ref(&self) -> &CacheKey {
        self
    }
}

#[derive(Clone)]
pub struct Cache {
    lru_cache: Arc<Mutex<LruCache<CacheKey, CachedProof>>>,
}

impl Cache {
    pub fn new(capacity: usize) -> Self {
        let lru_cache = LruCache::with_capacity(capacity);
        Cache {
            lru_cache: Arc::new(Mutex::new(lru_cache)),
        }
    }

    pub fn get<T: AsRef<CacheKey>>(&self, cache_key: T) -> Option<String> {
        let cache_key = cache_key.as_ref();
        let mut inner_cache = self.lru_cache.lock().unwrap();
        inner_cache.get(cache_key).map(|entry| entry.proof.clone())
    }

    pub fn set(&self, cache_key: CacheKey, proof: String) {
        let mut inner_cache = self.lru_cache.lock().unwrap();
        let entry = CachedProof { proof };
        inner_cache.insert(cache_key, entry);
    }
}
