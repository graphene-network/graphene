//! Iroh-backed dependency cache using the P2P network for blob storage.
//!
//! This implementation uses the [`P2PNetwork`] trait to store and retrieve
//! cached dependency images via content-addressed blob storage.
//!
//! ## Hash Mapping
//!
//! The cache maintains a mapping between cache keys (computed from requirements)
//! and blob hashes (computed from file contents). This is necessary because:
//! - Cache keys are deterministic based on build inputs
//! - Blob hashes are computed from actual file contents
//! - These differ, so we need an index to map between them

use super::{CacheError, DependencyCache};
use crate::metrics::{record_cache_hit, CacheLevel};
use crate::p2p::P2PNetwork;
use async_trait::async_trait;
use iroh_blobs::Hash;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Index entry for cache key to blob hash mapping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IndexEntry {
    /// The Iroh blob hash.
    blob_hash: String,
    /// Size in bytes.
    size: u64,
    /// Timestamp when cached.
    cached_at: u64,
}

/// A dependency cache backed by Iroh's content-addressed blob storage.
///
/// Images are stored as blobs and identified by their BLAKE3 hash.
/// This enables efficient P2P distribution of cached builds.
///
/// The cache maintains an index file mapping cache keys to blob hashes,
/// solving the mismatch between requirements-based keys and content hashes.
pub struct IrohCache<N: P2PNetwork> {
    /// The P2P network instance for blob operations.
    network: Arc<N>,

    /// Local storage path for exporting blobs to files.
    storage_path: PathBuf,

    /// Path to the index file.
    index_path: PathBuf,

    /// In-memory index cache.
    index: RwLock<HashMap<String, IndexEntry>>,
}

impl<N: P2PNetwork> IrohCache<N> {
    /// Create a new Iroh-backed cache with the given P2P network.
    pub fn new(network: Arc<N>, storage_path: PathBuf) -> Self {
        let index_path = storage_path.join("cache_index.json");
        Self {
            network,
            storage_path,
            index_path,
            index: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new cache and load the existing index.
    pub async fn with_loaded_index(network: Arc<N>, storage_path: PathBuf) -> Result<Self, CacheError> {
        let cache = Self::new(network, storage_path);
        cache.load_index().await?;
        Ok(cache)
    }

    /// Get the underlying P2P network.
    pub fn network(&self) -> &Arc<N> {
        &self.network
    }

    /// Load the index from disk.
    pub async fn load_index(&self) -> Result<(), CacheError> {
        if !self.index_path.exists() {
            return Ok(());
        }

        let contents = tokio::fs::read_to_string(&self.index_path)
            .await
            .map_err(|e| CacheError::IoError(e.to_string()))?;

        let loaded: HashMap<String, IndexEntry> = serde_json::from_str(&contents)
            .map_err(|e| CacheError::IoError(format!("Failed to parse index: {}", e)))?;

        let mut index = self.index.write().await;
        *index = loaded;

        tracing::debug!(entries = index.len(), "Loaded cache index");
        Ok(())
    }

    /// Save the index to disk.
    async fn save_index(&self) -> Result<(), CacheError> {
        let index = self.index.read().await;
        let contents = serde_json::to_string_pretty(&*index)
            .map_err(|e| CacheError::IoError(format!("Failed to serialize index: {}", e)))?;

        // Ensure directory exists
        if let Some(parent) = self.index_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CacheError::IoError(e.to_string()))?;
        }

        tokio::fs::write(&self.index_path, contents)
            .await
            .map_err(|e| CacheError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Look up a blob hash by cache key.
    pub async fn lookup_blob_hash(&self, cache_key: &str) -> Option<Hash> {
        let index = self.index.read().await;
        index.get(cache_key).and_then(|entry| {
            Self::parse_hash(&entry.blob_hash).ok()
        })
    }

    /// Store a mapping from cache key to blob hash.
    async fn store_mapping(&self, cache_key: &str, blob_hash: Hash, size: u64) -> Result<(), CacheError> {
        let entry = IndexEntry {
            blob_hash: hex::encode(blob_hash.as_bytes()),
            size,
            cached_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        {
            let mut index = self.index.write().await;
            index.insert(cache_key.to_string(), entry);
        }

        self.save_index().await
    }

    /// Convert a hex-encoded hash string to an Iroh Hash.
    fn parse_hash(hash_str: &str) -> Result<Hash, CacheError> {
        // BLAKE3 hashes are 32 bytes = 64 hex chars
        if hash_str.len() != 64 {
            return Err(CacheError::InvalidHash(format!(
                "Invalid hash length: expected 64 hex chars, got {}",
                hash_str.len()
            )));
        }

        let bytes = hex::decode(hash_str)
            .map_err(|e| CacheError::InvalidHash(format!("Invalid hex: {}", e)))?;

        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CacheError::InvalidHash("Failed to convert to 32-byte array".into()))?;

        Ok(Hash::from_bytes(array))
    }
}

#[async_trait]
impl<N: P2PNetwork + 'static> DependencyCache for IrohCache<N> {
    fn calculate_hash(&self, requirements: &[String]) -> String {
        // Sort requirements for deterministic hashing
        let mut sorted = requirements.to_vec();
        sorted.sort();

        // Hash the joined requirements using BLAKE3
        let input = sorted.join("|");
        let hash = blake3::hash(input.as_bytes());

        hex::encode(hash.as_bytes())
    }

    async fn get(&self, cache_key: &str) -> Result<Option<PathBuf>, CacheError> {
        // First, look up the blob hash in our index
        let blob_hash = match self.lookup_blob_hash(cache_key).await {
            Some(hash) => hash,
            None => {
                tracing::debug!(key = &cache_key[..8], "Cache key not in index");
                return Ok(None);
            }
        };

        // Check if we have it locally
        let has_locally = self
            .network
            .has_blob(blob_hash)
            .await
            .map_err(|e| CacheError::P2PError(e.to_string()))?;

        if has_locally {
            // Download/read the blob locally
            let data = self
                .network
                .download_blob(blob_hash, None)
                .await
                .map_err(|e| CacheError::P2PError(e.to_string()))?;

            // Export to a file for Firecracker
            let export_path = self.storage_path.join(format!("{}.unik", cache_key));
            std::fs::create_dir_all(&self.storage_path)
                .map_err(|e| CacheError::IoError(e.to_string()))?;

            std::fs::write(&export_path, &data)
                .map_err(|e| CacheError::IoError(e.to_string()))?;

            record_cache_hit(CacheLevel::Iroh);
            tracing::debug!(key = &cache_key[..8], "Cache hit (local blob)");
            return Ok(Some(export_path));
        }

        // Try to fetch from the P2P network
        tracing::debug!(key = &cache_key[..8], blob = %blob_hash, "Attempting P2P fetch");
        match self.network.download_blob(blob_hash, None).await {
            Ok(data) => {
                let export_path = self.storage_path.join(format!("{}.unik", cache_key));
                std::fs::create_dir_all(&self.storage_path)
                    .map_err(|e| CacheError::IoError(e.to_string()))?;

                std::fs::write(&export_path, &data)
                    .map_err(|e| CacheError::IoError(e.to_string()))?;

                record_cache_hit(CacheLevel::Iroh);
                tracing::info!(key = &cache_key[..8], "Cache hit (P2P fetch)");
                Ok(Some(export_path))
            }
            Err(e) => {
                tracing::debug!(key = &cache_key[..8], error = %e, "P2P fetch failed");
                Ok(None)
            }
        }
    }

    async fn put(&self, cache_key: &str, source_path: PathBuf) -> Result<PathBuf, CacheError> {
        // Get file size before upload
        let metadata = std::fs::metadata(&source_path)
            .map_err(|e| CacheError::IoError(e.to_string()))?;
        let size = metadata.len();

        // Upload the file to the P2P network
        let blob_hash = self
            .network
            .upload_blob_from_path(&source_path)
            .await
            .map_err(|e| CacheError::P2PError(e.to_string()))?;

        // Store the mapping from cache key to blob hash
        self.store_mapping(cache_key, blob_hash, size).await?;

        tracing::info!(
            key = &cache_key[..8],
            blob = %blob_hash,
            size = size,
            "Cached blob available"
        );

        // Return the original path (it's still valid for immediate use)
        Ok(source_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::MockGrapheneNode;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_calculate_hash_deterministic() {
        let network = Arc::new(MockGrapheneNode::new());
        let cache = IrohCache::new(network, PathBuf::from("/tmp"));

        let hash1 = cache.calculate_hash(&["pandas".into(), "numpy".into()]);
        let hash2 = cache.calculate_hash(&["numpy".into(), "pandas".into()]);

        // Order shouldn't matter (sorted internally)
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_cache_miss_returns_none() {
        let network = Arc::new(MockGrapheneNode::new());
        let temp = tempdir().unwrap();
        let cache = IrohCache::new(network, temp.path().to_path_buf());

        let hash = cache.calculate_hash(&["nonexistent".into()]);
        let result = cache.get(&hash).await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_put_creates_index_entry() {
        let network = Arc::new(MockGrapheneNode::new());
        let temp = tempdir().unwrap();
        let cache = IrohCache::new(network, temp.path().to_path_buf());

        // Create a test file
        let source = temp.path().join("test.unik");
        std::fs::write(&source, b"test unikernel data").unwrap();

        // Calculate hash for some requirements
        let cache_key = cache.calculate_hash(&["test".into()]);

        // Put the file
        cache.put(&cache_key, source.clone()).await.unwrap();

        // Verify index entry was created
        let blob_hash = cache.lookup_blob_hash(&cache_key).await;
        assert!(blob_hash.is_some());
    }

    #[tokio::test]
    async fn test_put_and_get_with_index() {
        let network = Arc::new(MockGrapheneNode::new());
        let temp = tempdir().unwrap();
        let cache = IrohCache::new(network, temp.path().to_path_buf());

        // Create a test file
        let source = temp.path().join("test.unik");
        std::fs::write(&source, b"test unikernel data").unwrap();

        // Calculate hash for some requirements
        let cache_key = cache.calculate_hash(&["test".into()]);

        // Put the file
        cache.put(&cache_key, source.clone()).await.unwrap();

        // Get should now find it via the index
        let result = cache.get(&cache_key).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_index_persistence() {
        let network = Arc::new(MockGrapheneNode::new());
        let temp = tempdir().unwrap();

        let cache_key;

        // Create and populate cache
        {
            let cache = IrohCache::new(Arc::clone(&network), temp.path().to_path_buf());

            let source = temp.path().join("test.unik");
            std::fs::write(&source, b"test data").unwrap();

            cache_key = cache.calculate_hash(&["persist".into()]);
            cache.put(&cache_key, source).await.unwrap();
        }

        // Create new cache and load index
        {
            let cache = IrohCache::with_loaded_index(network, temp.path().to_path_buf())
                .await
                .unwrap();

            let blob_hash = cache.lookup_blob_hash(&cache_key).await;
            assert!(blob_hash.is_some());
        }
    }
}
