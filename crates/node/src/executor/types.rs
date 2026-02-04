//! Core types for job execution.
//!
//! This module defines the request/response types and errors for the job executor.

use crate::cost::types::JobCostEstimate;
use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
use crate::p2p::protocol::types::JobAssets;
use iroh_blobs::Hash;
use std::time::Duration;
use thiserror::Error;

/// Reserved environment variable names injected by the executor.
///
/// These variables are automatically set for every job and cannot be overridden
/// by user-provided environment variables.
pub mod reserved_env {
    /// Unique job identifier (UUID format).
    pub const GRAPHENE_JOB_ID: &str = "GRAPHENE_JOB_ID";

    /// Path to the decrypted input data inside the unikernel.
    pub const GRAPHENE_INPUT_PATH: &str = "GRAPHENE_INPUT_PATH";

    /// Path where the job should write its output.
    pub const GRAPHENE_OUTPUT_PATH: &str = "GRAPHENE_OUTPUT_PATH";

    /// Maximum execution time in milliseconds.
    pub const GRAPHENE_TIMEOUT_MS: &str = "GRAPHENE_TIMEOUT_MS";

    /// All reserved environment variable names.
    pub const ALL: &[&str] = &[
        GRAPHENE_JOB_ID,
        GRAPHENE_INPUT_PATH,
        GRAPHENE_OUTPUT_PATH,
        GRAPHENE_TIMEOUT_MS,
    ];

    /// Returns true if the given name is a reserved environment variable.
    pub fn is_reserved(name: &str) -> bool {
        name.starts_with("GRAPHENE_")
    }
}

/// Request to execute a job.
///
/// Contains all information needed to run a job in an isolated unikernel environment.
#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    /// Unique job identifier.
    pub job_id: String,

    /// Resource requirements and configuration.
    pub manifest: JobManifest,

    /// Code and input blob references.
    pub assets: JobAssets,

    /// Ephemeral X25519 public key for forward secrecy.
    /// Used to derive the job decryption key.
    pub ephemeral_pubkey: [u8; 32],

    /// Solana PDA of the payment channel (for key derivation).
    pub channel_pda: [u8; 32],

    /// Payer's Ed25519 public key (for signature verification).
    pub payer_pubkey: [u8; 32],

    /// Requested result delivery mode (sync or async).
    pub delivery_mode: ResultDeliveryMode,

    /// Maximum cost estimate for this job (locked before execution).
    /// Used for cost settlement after completion.
    pub max_cost: Option<JobCostEstimate>,

    /// Client's node ID (Ed25519 public key) for downloading blobs.
    /// The executor uses this to download code/input blobs from the client.
    pub client_node_id: Option<[u8; 32]>,
}

impl ExecutionRequest {
    /// Creates a new execution request.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: impl Into<String>,
        manifest: JobManifest,
        assets: JobAssets,
        ephemeral_pubkey: [u8; 32],
        channel_pda: [u8; 32],
        payer_pubkey: [u8; 32],
        delivery_mode: ResultDeliveryMode,
    ) -> Self {
        Self {
            job_id: job_id.into(),
            manifest,
            assets,
            ephemeral_pubkey,
            channel_pda,
            payer_pubkey,
            delivery_mode,
            max_cost: None,
            client_node_id: None,
        }
    }

    /// Creates a new execution request with a cost estimate.
    #[allow(clippy::too_many_arguments)]
    pub fn with_cost(
        job_id: impl Into<String>,
        manifest: JobManifest,
        assets: JobAssets,
        ephemeral_pubkey: [u8; 32],
        channel_pda: [u8; 32],
        payer_pubkey: [u8; 32],
        delivery_mode: ResultDeliveryMode,
        max_cost: JobCostEstimate,
    ) -> Self {
        Self {
            job_id: job_id.into(),
            manifest,
            assets,
            ephemeral_pubkey,
            channel_pda,
            payer_pubkey,
            delivery_mode,
            max_cost: Some(max_cost),
            client_node_id: None,
        }
    }

    /// Sets the client node ID for blob downloads.
    pub fn with_client_node_id(mut self, client_node_id: [u8; 32]) -> Self {
        self.client_node_id = Some(client_node_id);
        self
    }

    /// Returns the timeout duration from the manifest.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.manifest.timeout_ms)
    }
}

/// Result of a successful job execution.
///
/// Contains the encrypted outputs and execution metadata.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Exit code of the unikernel (0 = success).
    pub exit_code: i32,

    /// Total execution duration.
    pub duration: Duration,

    /// Encrypted result/output data.
    pub encrypted_result: Vec<u8>,

    /// Encrypted stdout capture.
    pub encrypted_stdout: Vec<u8>,

    /// Encrypted stderr capture.
    pub encrypted_stderr: Vec<u8>,

    /// BLAKE3 hash of the encrypted result blob.
    pub result_hash: Hash,
}

impl ExecutionResult {
    /// Creates a new execution result.
    pub fn new(
        exit_code: i32,
        duration: Duration,
        encrypted_result: Vec<u8>,
        encrypted_stdout: Vec<u8>,
        encrypted_stderr: Vec<u8>,
        result_hash: Hash,
    ) -> Self {
        Self {
            exit_code,
            duration,
            encrypted_result,
            encrypted_stdout,
            encrypted_stderr,
            result_hash,
        }
    }

    /// Returns true if the job succeeded (exit code 0).
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    /// Returns the execution duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.duration.as_millis() as u64
    }
}

/// Errors that can occur during job execution.
#[derive(Debug, Error)]
pub enum ExecutionError {
    /// Failed to fetch code or input assets from Iroh or fallback URL.
    #[error("asset fetch failed: {0}")]
    AssetFetchFailed(String),

    /// Failed to decrypt job assets with the derived key.
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),

    /// Failed to look up cached unikernel build.
    #[error("cache lookup failed: {0}")]
    CacheLookupFailed(String),

    /// Failed to build the unikernel from Dockerfile/Kraftfile.
    #[error("build failed: {0}")]
    BuildFailed(String),

    /// Failed to create or mount the ext4 drive image.
    #[error("drive creation failed: {0}")]
    DriveFailed(String),

    /// Error from the VMM (Firecracker) during execution.
    #[error("VMM error: {0}")]
    VmmError(String),

    /// Failed to capture or encrypt job outputs.
    #[error("output processing failed: {0}")]
    OutputFailed(String),

    /// Job exceeded its configured timeout.
    #[error("execution timed out after {0:?}")]
    Timeout(Duration),

    /// Job was cancelled before completion.
    #[error("execution cancelled")]
    Cancelled,
}

impl ExecutionError {
    /// Creates an asset fetch error.
    pub fn asset_fetch(msg: impl Into<String>) -> Self {
        Self::AssetFetchFailed(msg.into())
    }

    /// Creates a decryption error.
    pub fn decryption(msg: impl Into<String>) -> Self {
        Self::DecryptionFailed(msg.into())
    }

    /// Creates a cache lookup error.
    pub fn cache_lookup(msg: impl Into<String>) -> Self {
        Self::CacheLookupFailed(msg.into())
    }

    /// Creates a build error.
    pub fn build(msg: impl Into<String>) -> Self {
        Self::BuildFailed(msg.into())
    }

    /// Creates a drive error.
    pub fn drive(msg: impl Into<String>) -> Self {
        Self::DriveFailed(msg.into())
    }

    /// Creates a VMM error.
    pub fn vmm(msg: impl Into<String>) -> Self {
        Self::VmmError(msg.into())
    }

    /// Creates an output processing error.
    pub fn output(msg: impl Into<String>) -> Self {
        Self::OutputFailed(msg.into())
    }

    /// Creates a timeout error.
    pub fn timeout(duration: Duration) -> Self {
        Self::Timeout(duration)
    }

    /// Returns true if this error indicates a worker fault (not user's fault).
    pub fn is_worker_fault(&self) -> bool {
        matches!(
            self,
            Self::VmmError(_) | Self::DriveFailed(_) | Self::CacheLookupFailed(_)
        )
    }

    /// Returns true if this error indicates a user fault.
    pub fn is_user_fault(&self) -> bool {
        matches!(
            self,
            Self::DecryptionFailed(_) | Self::BuildFailed(_) | Self::Timeout(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::EgressRule;
    use std::collections::HashMap;

    fn make_test_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    fn make_test_assets() -> JobAssets {
        JobAssets::blobs(
            Hash::from_bytes([1u8; 32]),
            Some(Hash::from_bytes([2u8; 32])),
        )
    }

    #[test]
    fn test_reserved_env_is_reserved() {
        assert!(reserved_env::is_reserved("GRAPHENE_JOB_ID"));
        assert!(reserved_env::is_reserved("GRAPHENE_CUSTOM"));
        assert!(!reserved_env::is_reserved("MY_VAR"));
        assert!(!reserved_env::is_reserved("GRAPHENE")); // No underscore, not reserved
    }

    #[test]
    fn test_execution_request_new() {
        let manifest = make_test_manifest();
        let assets = make_test_assets();

        let request = ExecutionRequest::new(
            "job-123",
            manifest.clone(),
            assets,
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            ResultDeliveryMode::Sync,
        );

        assert_eq!(request.job_id, "job-123");
        assert_eq!(request.manifest.vcpu, 2);
        assert_eq!(request.timeout(), Duration::from_millis(30000));
    }

    #[test]
    fn test_execution_result_succeeded() {
        let result = ExecutionResult::new(
            0,
            Duration::from_millis(1500),
            vec![1, 2, 3],
            vec![],
            vec![],
            Hash::from_bytes([0u8; 32]),
        );

        assert!(result.succeeded());
        assert_eq!(result.duration_ms(), 1500);
    }

    #[test]
    fn test_execution_result_failed() {
        let result = ExecutionResult::new(
            1,
            Duration::from_millis(500),
            vec![],
            vec![],
            vec![b'e', b'r', b'r'],
            Hash::from_bytes([0u8; 32]),
        );

        assert!(!result.succeeded());
    }

    #[test]
    fn test_execution_error_display() {
        let err = ExecutionError::asset_fetch("blob not found");
        assert_eq!(err.to_string(), "asset fetch failed: blob not found");

        let err = ExecutionError::timeout(Duration::from_secs(30));
        assert_eq!(err.to_string(), "execution timed out after 30s");

        let err = ExecutionError::Cancelled;
        assert_eq!(err.to_string(), "execution cancelled");
    }

    #[test]
    fn test_execution_error_fault_classification() {
        assert!(ExecutionError::vmm("crash").is_worker_fault());
        assert!(ExecutionError::drive("mount failed").is_worker_fault());
        assert!(!ExecutionError::vmm("crash").is_user_fault());

        assert!(ExecutionError::timeout(Duration::from_secs(30)).is_user_fault());
        assert!(ExecutionError::build("syntax error").is_user_fault());
        assert!(!ExecutionError::timeout(Duration::from_secs(30)).is_worker_fault());

        // Asset fetch is neither - could be network issue
        assert!(!ExecutionError::asset_fetch("timeout").is_worker_fault());
        assert!(!ExecutionError::asset_fetch("timeout").is_user_fault());
    }

    #[test]
    fn test_manifest_with_egress() {
        let manifest = JobManifest {
            vcpu: 1,
            memory_mb: 256,
            timeout_ms: 10000,
            kernel: "node:20".to_string(),
            egress_allowlist: vec![EgressRule {
                host: "api.example.com".to_string(),
                port: 443,
                protocol: "tcp".to_string(),
            }],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        };

        assert_eq!(manifest.egress_allowlist.len(), 1);
        assert_eq!(manifest.egress_allowlist[0].host, "api.example.com");
    }
}
