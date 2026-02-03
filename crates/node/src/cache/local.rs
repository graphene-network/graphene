use super::{CacheError, DependencyCache};
use crate::metrics::{record_cache_hit, record_cache_miss, CacheLevel};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;

pub struct LocalDiskCache {
    root_path: PathBuf,
}

impl LocalDiskCache {
    pub fn new(root: &str) -> Self {
        let path = PathBuf::from(root);
        // Ensure cache directory exists
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("Failed to create cache dir");
        }
        Self { root_path: path }
    }
}

#[async_trait]
impl DependencyCache for LocalDiskCache {
    fn calculate_hash(&self, requirements: &[String]) -> String {
        // 1. Sort to ensure determinism
        let mut sorted = requirements.to_vec();
        sorted.sort();

        // 2. Join strings
        let payload = sorted.join("|");

        // 3. Hash using BLAKE3 (matches Iroh's content addressing)
        hex::encode(blake3::hash(payload.as_bytes()).as_bytes())
    }

    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError> {
        let path = self.root_path.join(format!("{}.ext4", hash));

        if path.exists() {
            record_cache_hit(CacheLevel::Local);
            tracing::debug!(hash = &hash[0..8], "Cache hit (local)");
            Ok(Some(path))
        } else {
            record_cache_miss(CacheLevel::Local);
            tracing::debug!(hash = &hash[0..8], "Cache miss (local)");
            Ok(None)
        }
    }

    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        let dest_path = self.root_path.join(format!("{}.ext4", hash));

        tracing::debug!(path = ?dest_path, "Saving new cache layer");

        // Move the temp file to the permanent cache
        fs::rename(source_path, &dest_path)
            .await
            .map_err(|e| CacheError::IoError(e.to_string()))?;

        Ok(dest_path)
    }
}
