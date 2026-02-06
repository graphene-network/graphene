//! BuildCache trait and LayeredBuildCache implementation.
//!
//! Provides a unified interface for the L1/L2/L3 cache hierarchy,
//! combining local disk cache with Iroh P2P distribution.

use async_trait::async_trait;
use iroh_blobs::Hash;
use std::path::PathBuf;
use std::sync::Arc;

use super::iroh::IrohCache;
use super::keys::full_build_key;
use super::local::LocalDiskCache;
use super::{CacheError, DependencyCache};
use crate::metrics::{record_cache_hit, record_cache_miss, CacheLevel};
use crate::p2p::{P2PNetwork, TopicId};

/// Result of a cache lookup, including which layer it came from.
#[derive(Debug, Clone)]
pub struct CacheLookupResult {
    /// Path to the cached artifact.
    pub path: PathBuf,
    /// Which cache layer provided the hit.
    pub level: CacheLevel,
}

/// Trait for build artifact caching.
///
/// Implementations provide lookup, storage, and P2P announcement
/// of cached unikernel builds.
#[async_trait]
pub trait BuildCache: Send + Sync {
    /// Look up a cached build by its inputs.
    ///
    /// Checks L3 (full build) cache first locally, then via Iroh P2P.
    /// Returns the cached artifact path and which level it came from.
    async fn lookup(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
    ) -> Result<Option<CacheLookupResult>, CacheError>;

    /// Store a build artifact in the cache.
    ///
    /// Stores locally and uploads to Iroh P2P for distribution.
    /// Returns the blob hash for P2P retrieval.
    async fn store(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
        artifact_path: PathBuf,
    ) -> Result<Hash, CacheError>;

    /// Announce cache availability to the P2P network.
    ///
    /// Broadcasts a CacheAnnouncement on the cache gossip topic
    /// so other nodes know this build is available.
    async fn announce(
        &self,
        cache_key: &[u8; 32],
        blob_hash: Hash,
        size_bytes: u64,
        runtime_spec: &str,
    ) -> Result<(), CacheError>;
}

/// Layered build cache combining local storage with Iroh P2P.
///
/// Lookup order:
/// 1. Local disk cache (fastest)
/// 2. Iroh P2P network (distributed)
/// 3. Cache miss → rebuild required
pub struct LayeredBuildCache<N: P2PNetwork> {
    local: LocalDiskCache,
    iroh: IrohCache<N>,
    network: Arc<N>,
}

impl<N: P2PNetwork + 'static> LayeredBuildCache<N> {
    /// Create a new layered build cache.
    pub fn new(local: LocalDiskCache, iroh: IrohCache<N>, network: Arc<N>) -> Self {
        Self {
            local,
            iroh,
            network,
        }
    }

    /// Compute the L3 cache key from inputs.
    fn compute_cache_key(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
    ) -> [u8; 32] {
        let code_hash = blake3::Hash::from_bytes(*code_hash);
        *full_build_key(runtime_spec, requirements, &code_hash).as_bytes()
    }
}

#[async_trait]
impl<N: P2PNetwork + 'static> BuildCache for LayeredBuildCache<N> {
    async fn lookup(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
    ) -> Result<Option<CacheLookupResult>, CacheError> {
        let cache_key = self.compute_cache_key(runtime_spec, requirements, code_hash);
        let cache_key_hex = hex::encode(cache_key);

        // Try local cache first
        if let Some(path) = self.local.get(&cache_key_hex).await? {
            tracing::debug!(key = &cache_key_hex[..8], "L3 cache hit (local)");
            record_cache_hit(CacheLevel::L3Local);
            return Ok(Some(CacheLookupResult {
                path,
                level: CacheLevel::L3Local,
            }));
        }

        // Try Iroh P2P cache
        if let Some(path) = self.iroh.get(&cache_key_hex).await? {
            tracing::debug!(key = &cache_key_hex[..8], "L3 cache hit (iroh)");
            record_cache_hit(CacheLevel::L3Iroh);
            return Ok(Some(CacheLookupResult {
                path,
                level: CacheLevel::L3Iroh,
            }));
        }

        // Cache miss - rebuild required
        tracing::debug!(key = &cache_key_hex[..8], "L3 cache miss");
        record_cache_miss(CacheLevel::L3Rebuild);
        Ok(None)
    }

    async fn store(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
        artifact_path: PathBuf,
    ) -> Result<Hash, CacheError> {
        let cache_key = self.compute_cache_key(runtime_spec, requirements, code_hash);
        let cache_key_hex = hex::encode(cache_key);

        // Store in local cache (moves the file)
        let local_path = self.local.put(&cache_key_hex, artifact_path).await?;
        tracing::debug!(key = &cache_key_hex[..8], "Stored in local cache");

        // Upload to Iroh P2P
        let _iroh_path = self.iroh.put(&cache_key_hex, local_path.clone()).await?;

        // Get the blob hash from the index (stored during put)
        let blob_hash = self
            .iroh
            .lookup_blob_hash(&cache_key_hex)
            .await
            .ok_or_else(|| CacheError::P2PError("Failed to get blob hash after upload".into()))?;

        tracing::debug!(
            key = &cache_key_hex[..8],
            blob = %blob_hash,
            "Uploaded to Iroh P2P"
        );

        // Announce availability
        let metadata = std::fs::metadata(&local_path)?;
        self.announce(&cache_key, blob_hash, metadata.len(), runtime_spec)
            .await?;

        Ok(blob_hash)
    }

    async fn announce(
        &self,
        cache_key: &[u8; 32],
        blob_hash: Hash,
        size_bytes: u64,
        runtime_spec: &str,
    ) -> Result<(), CacheError> {
        use crate::p2p::messages::CacheAnnouncement;

        let announcement = CacheAnnouncement {
            cache_key: *cache_key,
            blob_hash,
            size_bytes,
            runtime_spec: runtime_spec.to_string(),
        };

        let message =
            serde_json::to_vec(&announcement).map_err(|e| CacheError::IoError(e.to_string()))?;

        self.network
            .broadcast(TopicId::cache_v1(), &message)
            .await
            .map_err(|e| CacheError::P2PError(e.to_string()))?;

        tracing::info!(
            blob = %blob_hash,
            size = size_bytes,
            kernel = runtime_spec,
            "Announced cache availability"
        );

        Ok(())
    }
}

/// Mock build cache for testing.
#[derive(Default)]
pub struct MockBuildCache {
    entries: std::sync::RwLock<std::collections::HashMap<[u8; 32], PathBuf>>,
    announcements: std::sync::RwLock<Vec<([u8; 32], Hash)>>,
}

impl MockBuildCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the number of cache entries.
    pub fn entry_count(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    /// Get the number of announcements made.
    pub fn announcement_count(&self) -> usize {
        self.announcements.read().unwrap().len()
    }

    /// Pre-populate with a cache entry.
    pub fn insert(&self, key: [u8; 32], path: PathBuf) {
        self.entries.write().unwrap().insert(key, path);
    }
}

#[async_trait]
impl BuildCache for MockBuildCache {
    async fn lookup(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
    ) -> Result<Option<CacheLookupResult>, CacheError> {
        let code_hash = blake3::Hash::from_bytes(*code_hash);
        let cache_key = *full_build_key(runtime_spec, requirements, &code_hash).as_bytes();

        let entries = self.entries.read().unwrap();
        if let Some(path) = entries.get(&cache_key) {
            Ok(Some(CacheLookupResult {
                path: path.clone(),
                level: CacheLevel::L3Local,
            }))
        } else {
            Ok(None)
        }
    }

    async fn store(
        &self,
        runtime_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
        artifact_path: PathBuf,
    ) -> Result<Hash, CacheError> {
        let code_hash_blake = blake3::Hash::from_bytes(*code_hash);
        let cache_key = *full_build_key(runtime_spec, requirements, &code_hash_blake).as_bytes();

        self.entries
            .write()
            .unwrap()
            .insert(cache_key, artifact_path);

        // Return a mock hash
        Ok(Hash::from_bytes(cache_key))
    }

    async fn announce(
        &self,
        cache_key: &[u8; 32],
        blob_hash: Hash,
        _size_bytes: u64,
        _runtime_spec: &str,
    ) -> Result<(), CacheError> {
        self.announcements
            .write()
            .unwrap()
            .push((*cache_key, blob_hash));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::keys::hash_bytes;

    #[tokio::test]
    async fn test_mock_cache_miss() {
        let cache = MockBuildCache::new();
        let code_hash = hash_bytes(b"test code");

        let result = cache
            .lookup("python:3.12", &["pandas".into()], code_hash.as_bytes())
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_cache_store_and_lookup() {
        let cache = MockBuildCache::new();
        let code_hash = hash_bytes(b"test code");

        // Store
        cache
            .store(
                "python:3.12",
                &["pandas".into()],
                code_hash.as_bytes(),
                PathBuf::from("/tmp/test.unik"),
            )
            .await
            .unwrap();

        // Lookup should hit
        let result = cache
            .lookup("python:3.12", &["pandas".into()], code_hash.as_bytes())
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().path, PathBuf::from("/tmp/test.unik"));
    }

    #[tokio::test]
    async fn test_mock_cache_announcements() {
        let cache = MockBuildCache::new();
        let key = [0u8; 32];
        let hash = Hash::from_bytes([1u8; 32]);

        cache
            .announce(&key, hash, 1024, "python:3.12")
            .await
            .unwrap();

        assert_eq!(cache.announcement_count(), 1);
    }
}
