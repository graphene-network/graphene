//! Core types for job execution.

use crate::types::{JobAssets, JobManifest};
use std::time::Duration;
use thiserror::Error;

/// Reserved environment variable names injected by the executor.
pub mod reserved_env {
    pub const GRAPHENE_JOB_ID: &str = "GRAPHENE_JOB_ID";
    pub const GRAPHENE_INPUT_PATH: &str = "GRAPHENE_INPUT_PATH";
    pub const GRAPHENE_OUTPUT_PATH: &str = "GRAPHENE_OUTPUT_PATH";
    pub const GRAPHENE_TIMEOUT_MS: &str = "GRAPHENE_TIMEOUT_MS";

    pub const ALL: &[&str] = &[
        GRAPHENE_JOB_ID,
        GRAPHENE_INPUT_PATH,
        GRAPHENE_OUTPUT_PATH,
        GRAPHENE_TIMEOUT_MS,
    ];

    pub fn is_reserved(name: &str) -> bool {
        name.starts_with("GRAPHENE_")
    }
}

/// Request to execute a job.
#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    /// Unique job identifier.
    pub job_id: String,
    /// Resource requirements and configuration.
    pub manifest: JobManifest,
    /// Code and input assets.
    pub assets: JobAssets,
}

impl ExecutionRequest {
    pub fn new(
        job_id: impl Into<String>,
        manifest: JobManifest,
        assets: JobAssets,
    ) -> Self {
        Self {
            job_id: job_id.into(),
            manifest,
            assets,
        }
    }

    /// Returns the timeout duration from the manifest.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.manifest.timeout_ms)
    }
}

/// Result of a successful job execution.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Exit code of the unikernel (0 = success).
    pub exit_code: i32,
    /// Total execution duration.
    pub duration: Duration,
    /// Result/output data (tarball of /output directory).
    pub result: Vec<u8>,
    /// Captured stdout.
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
    /// BLAKE3 hash of the result.
    pub result_hash: [u8; 32],
}

impl ExecutionResult {
    pub fn new(
        exit_code: i32,
        duration: Duration,
        result: Vec<u8>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> Self {
        let result_hash = *blake3::hash(&result).as_bytes();
        Self {
            exit_code,
            duration,
            result,
            stdout,
            stderr,
            result_hash,
        }
    }

    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration.as_millis() as u64
    }
}

/// Errors that can occur during job execution.
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("asset fetch failed: {0}")]
    AssetFetchFailed(String),

    #[error("decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("cache lookup failed: {0}")]
    CacheLookupFailed(String),

    #[error("build failed: {0}")]
    BuildFailed(String),

    #[error("drive creation failed: {0}")]
    DriveFailed(String),

    #[error("VMM error: {0}")]
    VmmError(String),

    #[error("output processing failed: {0}")]
    OutputFailed(String),

    #[error("execution timed out after {0:?}")]
    Timeout(Duration),

    #[error("execution cancelled")]
    Cancelled,
}

impl ExecutionError {
    pub fn asset_fetch(msg: impl Into<String>) -> Self {
        Self::AssetFetchFailed(msg.into())
    }
    pub fn decompression(msg: impl Into<String>) -> Self {
        Self::DecompressionFailed(msg.into())
    }
    pub fn cache_lookup(msg: impl Into<String>) -> Self {
        Self::CacheLookupFailed(msg.into())
    }
    pub fn build(msg: impl Into<String>) -> Self {
        Self::BuildFailed(msg.into())
    }
    pub fn drive(msg: impl Into<String>) -> Self {
        Self::DriveFailed(msg.into())
    }
    pub fn vmm(msg: impl Into<String>) -> Self {
        Self::VmmError(msg.into())
    }
    pub fn output(msg: impl Into<String>) -> Self {
        Self::OutputFailed(msg.into())
    }
    pub fn timeout(duration: Duration) -> Self {
        Self::Timeout(duration)
    }

    pub fn is_worker_fault(&self) -> bool {
        matches!(
            self,
            Self::VmmError(_) | Self::DriveFailed(_) | Self::CacheLookupFailed(_)
        )
    }

    pub fn is_user_fault(&self) -> bool {
        matches!(
            self,
            Self::DecompressionFailed(_) | Self::BuildFailed(_) | Self::Timeout(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EgressRule;
    use std::collections::HashMap;

    fn make_test_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30000,
            runtime: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    fn make_test_assets() -> JobAssets {
        JobAssets::inline(b"print('hello')".to_vec(), None)
    }

    #[test]
    fn test_reserved_env_is_reserved() {
        assert!(reserved_env::is_reserved("GRAPHENE_JOB_ID"));
        assert!(reserved_env::is_reserved("GRAPHENE_CUSTOM"));
        assert!(!reserved_env::is_reserved("MY_VAR"));
        assert!(!reserved_env::is_reserved("GRAPHENE"));
    }

    #[test]
    fn test_execution_request_new() {
        let manifest = make_test_manifest();
        let assets = make_test_assets();

        let request = ExecutionRequest::new("job-123", manifest, assets);

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
    }

    #[test]
    fn test_manifest_with_egress() {
        let manifest = JobManifest {
            vcpu: 1,
            memory_mb: 256,
            timeout_ms: 10000,
            runtime: "node:20".to_string(),
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
