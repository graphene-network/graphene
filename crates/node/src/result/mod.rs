//! Result delivery module for returning job results to users.
//!
//! This module implements dual-mode result delivery:
//! - **Sync mode** (default): Stream results directly over QUIC for lowest latency (~10ms)
//! - **Async mode**: Upload to Iroh blob store for offline retrieval (24h TTL)
//!
//! # Architecture
//!
//! ```text
//! Job Completes → Encrypt Result → [Sync or Async delivery]
//!
//! Sync (default):                    Async (opt-in/fallback):
//!   Worker streams directly            Worker uploads to Iroh
//!   over QUIC to user                  User fetches by hash
//!   SUCCEEDED → DELIVERED              SUCCEEDED → DELIVERING → DELIVERED
//!   (~10ms latency)                    (24h TTL)
//! ```
//!
//! # Example
//!
//! ```text
//! // Pseudocode showing the delivery flow
//! let delivery = MockResultDelivery::new();  // or SyncDelivery/AsyncDelivery
//!
//! // Attempt sync delivery with automatic fallback to async
//! let outcome = delivery.deliver(job_id, &encrypted_result, mode, user_addr, true).await?;
//!
//! match outcome {
//!     DeliveryOutcome::SyncDelivered => println!("Streamed directly"),
//!     DeliveryOutcome::AsyncUploaded { result_hash, .. } => {
//!         println!("Uploaded to {}", result_hash)
//!     }
//! }
//! ```

pub mod async_delivery;
pub mod mock;
pub mod sync;

use async_trait::async_trait;
use iroh::EndpointAddr;
use iroh_blobs::Hash;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::p2p::messages::ResultDeliveryMode;

/// Errors that can occur during result delivery.
#[derive(Debug)]
pub enum DeliveryError {
    /// Failed to encrypt result data
    EncryptionError(String),
    /// Failed to upload blob to Iroh
    BlobUploadError(String),
    /// Failed to connect to user for sync delivery
    ConnectionError(String),
    /// Failed to stream result over QUIC
    StreamError(String),
    /// User is offline (for sync mode)
    UserOffline,
    /// Delivery timed out
    Timeout,
    /// Generic I/O error
    IoError(std::io::Error),
}

impl Error for DeliveryError {}

impl Display for DeliveryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliveryError::EncryptionError(msg) => write!(f, "Encryption error: {}", msg),
            DeliveryError::BlobUploadError(msg) => write!(f, "Blob upload error: {}", msg),
            DeliveryError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            DeliveryError::StreamError(msg) => write!(f, "Stream error: {}", msg),
            DeliveryError::UserOffline => write!(f, "User is offline"),
            DeliveryError::Timeout => write!(f, "Delivery timed out"),
            DeliveryError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl From<std::io::Error> for DeliveryError {
    fn from(e: std::io::Error) -> Self {
        DeliveryError::IoError(e)
    }
}

/// Outcome of a delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Result was delivered synchronously via QUIC stream.
    SyncDelivered,
    /// Result was uploaded to Iroh blob store for async retrieval.
    AsyncUploaded {
        /// Hash of the encrypted result blob.
        result_hash: Hash,
        /// Hash of the encrypted stdout blob.
        stdout_hash: Hash,
        /// Hash of the encrypted stderr blob.
        stderr_hash: Hash,
    },
}

impl DeliveryOutcome {
    /// Returns true if delivery was synchronous.
    pub fn is_sync(&self) -> bool {
        matches!(self, DeliveryOutcome::SyncDelivered)
    }

    /// Returns true if delivery was asynchronous (blob upload).
    pub fn is_async(&self) -> bool {
        matches!(self, DeliveryOutcome::AsyncUploaded { .. })
    }
}

/// Encrypted job result ready for delivery.
#[derive(Debug, Clone)]
pub struct EncryptedResult {
    /// Encrypted return value/output data.
    pub result: Vec<u8>,
    /// Encrypted stdout capture.
    pub stdout: Vec<u8>,
    /// Encrypted stderr capture.
    pub stderr: Vec<u8>,
    /// Exit code of the job.
    pub exit_code: i32,
    /// Execution time in milliseconds.
    pub execution_ms: u64,
}

/// Trait for delivering job results to users.
///
/// Implementations handle the actual transport mechanism (QUIC streaming
/// or Iroh blob upload).
#[async_trait]
pub trait ResultDelivery: Send + Sync {
    /// Deliver a result using sync mode (direct QUIC streaming).
    ///
    /// # Arguments
    ///
    /// * `job_id` - The job identifier
    /// * `result` - The encrypted result data
    /// * `user_addr` - The user's network address for direct connection
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if delivery succeeded, or an error if the user
    /// is offline or the connection fails.
    async fn deliver_sync(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        user_addr: &EndpointAddr,
    ) -> Result<(), DeliveryError>;

    /// Deliver a result using async mode (Iroh blob upload).
    ///
    /// # Arguments
    ///
    /// * `job_id` - The job identifier
    /// * `result` - The encrypted result data
    ///
    /// # Returns
    ///
    /// Returns the blob hashes for the uploaded result, stdout, and stderr.
    async fn deliver_async(
        &self,
        job_id: &str,
        result: &EncryptedResult,
    ) -> Result<(Hash, Hash, Hash), DeliveryError>;

    /// Attempt delivery with the specified mode, with optional fallback.
    ///
    /// For sync mode, if delivery fails due to user being offline,
    /// automatically falls back to async mode.
    ///
    /// # Arguments
    ///
    /// * `job_id` - The job identifier
    /// * `result` - The encrypted result data
    /// * `mode` - Requested delivery mode
    /// * `user_addr` - User's address (required for sync, optional for async)
    /// * `fallback` - Whether to fall back to async if sync fails
    async fn deliver(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        mode: ResultDeliveryMode,
        user_addr: Option<&EndpointAddr>,
        fallback: bool,
    ) -> Result<DeliveryOutcome, DeliveryError>;
}

pub use async_delivery::AsyncDelivery;
pub use mock::MockResultDelivery;
pub use sync::SyncDelivery;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_outcome_is_sync() {
        let sync = DeliveryOutcome::SyncDelivered;
        assert!(sync.is_sync());
        assert!(!sync.is_async());
    }

    #[test]
    fn test_delivery_outcome_is_async() {
        let hash = Hash::new(b"test");
        let async_outcome = DeliveryOutcome::AsyncUploaded {
            result_hash: hash,
            stdout_hash: hash,
            stderr_hash: hash,
        };
        assert!(async_outcome.is_async());
        assert!(!async_outcome.is_sync());
    }

    #[test]
    fn test_delivery_error_display() {
        assert_eq!(DeliveryError::UserOffline.to_string(), "User is offline");
        assert_eq!(DeliveryError::Timeout.to_string(), "Delivery timed out");
        assert_eq!(
            DeliveryError::ConnectionError("refused".to_string()).to_string(),
            "Connection error: refused"
        );
    }
}
