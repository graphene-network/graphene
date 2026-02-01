use super::{CacheError, DependencyCache};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MockCache {
    store: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl MockCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Helper for tests: Pre-populate the cache to simulate a "Hit"
    pub fn preload(&self, hash: &str, path: PathBuf) {
        let mut store = self.store.lock().unwrap();
        store.insert(hash.to_string(), path);
    }
}

#[async_trait]
impl DependencyCache for MockCache {
    fn calculate_hash(&self, requirements: &[String]) -> String {
        // Simple mock hash: just join the strings.
        // We don't need real SHA256 for logic testing.
        format!("mock_hash_{}", requirements.join("_"))
    }

    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError> {
        let store = self.store.lock().unwrap();
        if let Some(path) = store.get(hash) {
            println!("💾 [MOCK CACHE] Hit! Found: {:?}", path);
            Ok(Some(path.clone()))
        } else {
            println!("💾 [MOCK CACHE] Miss. Hash not found: {}", hash);
            Ok(None)
        }
    }

    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        let mut store = self.store.lock().unwrap();

        // In a real cache, we'd move the file.
        // In the mock, we just remember the path.
        println!("💾 [MOCK CACHE] Saving: {} -> {:?}", hash, source_path);
        store.insert(hash.to_string(), source_path.clone());

        Ok(source_path)
    }
}
