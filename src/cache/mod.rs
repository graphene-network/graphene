use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub mod local;
pub mod mock;

#[derive(Debug)]
pub enum CacheError {
    IoError(String),
    ComputeError(String),
}

impl Error for CacheError {
    fn description(&self) -> &str {
        match self {
            CacheError::IoError(msg) => msg,
            CacheError::ComputeError(msg) => msg,
        }
    }
}

impl Display for CacheError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::IoError(msg) => write!(f, "IO Error: {}", msg),
            CacheError::ComputeError(msg) => write!(f, "Compute Error: {}", msg),
        }
    }
}

#[async_trait]
pub trait DependencyCache: Send + Sync {
    /// 1. Takes the raw dependency list.
    /// 2. Returns the SHA256 Hash (The "Key").
    fn calculate_hash(&self, requirements: &[String]) -> String;

    /// 1. Checks if the Key exists in the store.
    /// 2. If YES: Returns the PathBuf to the image.
    /// 3. If NO: Returns None (Caller must trigger a build).
    async fn get(&self, hash: &str) -> Result<Option<PathBuf>, CacheError>;

    /// 1. Takes a newly built image path.
    /// 2. Moves/Copies it into the permanent store under the Hash Key.
    async fn put(&self, hash: &str, source_path: PathBuf) -> Result<PathBuf, CacheError>;
}
