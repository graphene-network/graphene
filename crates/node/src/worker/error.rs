//! Error types for the worker binary.

use crate::p2p::P2PError;
use thiserror::Error;

use super::state::StateError;

/// Errors that can occur during worker operations.
#[derive(Debug, Error)]
pub enum WorkerError {
    /// Configuration file error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// P2P networking error
    #[error("P2P error: {0}")]
    P2PError(#[from] P2PError),

    /// Solana RPC or program error
    #[error("Solana error: {0}")]
    SolanaError(String),

    /// VMM/Firecracker error
    #[error("VMM error: {0}")]
    VmmError(String),

    /// Worker is already running
    #[error("Worker is already running")]
    AlreadyRunning,

    /// Worker is shutting down
    #[error("Worker is shutting down")]
    ShuttingDown,

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// TOML parsing error
    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),

    /// State machine error
    #[error("State error: {0}")]
    StateError(#[from] StateError),
}

impl From<solana_sdk::signer::SignerError> for WorkerError {
    fn from(e: solana_sdk::signer::SignerError) -> Self {
        WorkerError::SolanaError(e.to_string())
    }
}

impl From<solana_client::client_error::ClientError> for WorkerError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        WorkerError::SolanaError(e.to_string())
    }
}
