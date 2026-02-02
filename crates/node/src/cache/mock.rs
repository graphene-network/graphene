use super::{CacheError, DependencyCache};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
pub struct CacheSpyState {
    pub get_calls: usize,
    pub put_calls: usize,
    pub hits: usize,
    pub misses: usize,
}

#[derive(Clone)]
pub struct MockCache {
    store: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub spy: Arc<Mutex<CacheSpyState>>,
}

impl Default for MockCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
            spy: Arc::new(Mutex::new(CacheSpyState::default())),
        }
    }

    // --- SPY METHODS ---
    pub fn get_hit_count(&self) -> usize {
        self.spy.lock().unwrap().hits
    }

    pub fn get_miss_count(&self) -> usize {
        self.spy.lock().unwrap().misses
    }
}

#[async_trait]
impl DependencyCache for MockCache {
    fn calculate_hash(&self, requirements: &[String]) -> String {
        format!("hash_{}", requirements.join("_"))
    }

    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError> {
        let mut spy = self.spy.lock().unwrap();
        spy.get_calls += 1;

        let store = self.store.lock().unwrap();
        if let Some(path) = store.get(hash) {
            spy.hits += 1;
            Ok(Some(path.clone()))
        } else {
            spy.misses += 1;
            Ok(None)
        }
    }

    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        self.spy.lock().unwrap().put_calls += 1;
        self.store
            .lock()
            .unwrap()
            .insert(hash.to_string(), source_path.clone());
        Ok(source_path)
    }
}
