use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub mod local;
pub mod matrix;
pub mod mock;
pub mod types;

pub use types::{Architecture, KernelMetadata, KernelSpec, Runtime};

/// Errors that can occur during kernel operations
#[derive(Debug)]
pub enum KernelError {
    /// Kernel specification could not be parsed
    InvalidSpec(String),
    /// Requested kernel not found in registry
    NotFound(String),
    /// IO error during kernel operations
    IoError(String),
    /// Network error during kernel download
    NetworkError(String),
    /// Hash verification failed
    HashMismatch { expected: String, actual: String },
    /// Configuration error (e.g., invalid matrix.toml)
    ConfigError(String),
}

impl Error for KernelError {}

impl Display for KernelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KernelError::InvalidSpec(s) => write!(f, "invalid kernel spec: {}", s),
            KernelError::NotFound(s) => write!(f, "kernel not found: {}", s),
            KernelError::IoError(s) => write!(f, "IO error: {}", s),
            KernelError::NetworkError(s) => write!(f, "network error: {}", s),
            KernelError::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch: expected {}, got {}", expected, actual)
            }
            KernelError::ConfigError(s) => write!(f, "config error: {}", s),
        }
    }
}

impl From<std::io::Error> for KernelError {
    fn from(err: std::io::Error) -> Self {
        KernelError::IoError(err.to_string())
    }
}

impl From<reqwest::Error> for KernelError {
    fn from(err: reqwest::Error) -> Self {
        KernelError::NetworkError(err.to_string())
    }
}

/// Registry for managing pre-built unikernels
///
/// Implementations provide kernel lookup, caching, and download capabilities.
/// Follows the same trait pattern as `DependencyCache`.
#[async_trait]
pub trait KernelRegistry: Send + Sync {
    /// Parse a kernel name string (e.g., "python-3.11") into a KernelSpec
    fn resolve(&self, name: &str) -> Result<KernelSpec, KernelError>;

    /// Check if kernel is cached locally, returns path if available
    async fn get(&self, spec: &KernelSpec) -> Result<Option<PathBuf>, KernelError>;

    /// Ensure kernel is available locally, downloading if necessary
    async fn ensure(&self, spec: &KernelSpec) -> Result<PathBuf, KernelError>;

    /// List all kernels defined in the version matrix
    fn list_available(&self) -> Vec<KernelSpec>;

    /// Get metadata for a kernel (memory requirements, boot args, etc.)
    fn get_metadata(&self, spec: &KernelSpec) -> Result<KernelMetadata, KernelError>;

    /// Get boot arguments for Firecracker
    fn get_boot_args(&self, spec: &KernelSpec) -> String {
        self.get_metadata(spec)
            .map(|m| m.boot_args())
            .unwrap_or_else(|_| {
                "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules".to_string()
            })
    }
}
