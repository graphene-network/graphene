use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub mod keys;
pub mod local;
pub mod mock;

pub use keys::{full_build_key, hash_bytes, l1_key, l2_key, l3_key};

/// Cache level indicating where a hit was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLevel {
    /// L1: exact match (runtime + deps + code hash)
    L1,
    /// L2: runtime + deps match (different code)
    L2,
    /// L3: runtime match only
    L3,
}

/// Result of a successful cache lookup.
#[derive(Debug, Clone)]
pub struct CacheLookupResult {
    /// Path to the cached kernel binary.
    pub path: PathBuf,
    /// Which cache level matched.
    pub level: CacheLevel,
}

/// Trait for looking up cached unikernel builds.
#[async_trait]
pub trait BuildCache: Send + Sync {
    /// Look up a cached kernel for the given spec.
    async fn lookup(
        &self,
        kernel_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
    ) -> Result<Option<CacheLookupResult>, CacheError>;

    /// Store a built kernel in the cache.
    async fn store(
        &self,
        kernel_spec: &str,
        requirements: &[String],
        code_hash: &[u8; 32],
        kernel_path: &std::path::Path,
    ) -> Result<PathBuf, CacheError>;
}

#[derive(Debug)]
pub enum CacheError {
    IoError(String),
    ComputeError(String),
    InvalidHash(String),
    P2PError(String),
}

impl Error for CacheError {
    fn description(&self) -> &str {
        match self {
            CacheError::IoError(msg) => msg,
            CacheError::ComputeError(msg) => msg,
            CacheError::InvalidHash(msg) => msg,
            CacheError::P2PError(msg) => msg,
        }
    }
}

impl Display for CacheError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::IoError(msg) => write!(f, "IO Error: {}", msg),
            CacheError::ComputeError(msg) => write!(f, "Compute Error: {}", msg),
            CacheError::InvalidHash(msg) => write!(f, "Invalid Hash: {}", msg),
            CacheError::P2PError(msg) => write!(f, "P2P Error: {}", msg),
        }
    }
}

impl From<std::io::Error> for CacheError {
    fn from(err: std::io::Error) -> Self {
        CacheError::IoError(err.to_string())
    }
}

#[async_trait]
pub trait DependencyCache: Send + Sync {
    /// Compute a deterministic BLAKE3 hash of the requirements list.
    ///
    /// Requirements are sorted before hashing for determinism.
    fn calculate_hash(&self, requirements: &[String]) -> String;

    /// 1. Checks if the Key exists in the store.
    /// 2. If YES: Returns the PathBuf to the image.
    /// 3. If NO: Returns None (Caller must trigger a build).
    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError>;

    /// 1. Takes a newly built image path.
    /// 2. Moves/Copies it into the permanent store under the Hash Key.
    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError>;
}
