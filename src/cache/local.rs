use super::{CacheError, DependencyCache};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
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

        // 3. Hash
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hex::encode(hasher.finalize())
    }

    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError> {
        let path = self.root_path.join(format!("{}.ext4", hash));

        if path.exists() {
            println!("✅ [CACHE] Hit! Found layer for {}", &hash[0..8]);
            Ok(Some(path))
        } else {
            println!("❌ [CACHE] Miss. Layer {} not found.", &hash[0..8]);
            Ok(None)
        }
    }

    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        let dest_path = self.root_path.join(format!("{}.ext4", hash));

        println!("💾 [CACHE] Saving new layer to {:?}", dest_path);

        // Move the temp file to the permanent cache
        fs::rename(source_path, &dest_path)
            .await
            .map_err(|e| CacheError::IoError(e.to_string()))?;

        Ok(dest_path)
    }
}
