//! HTTP API request and response types for the OpenCapsule worker node.
//!
//! These types define the JSON contract between clients (SDK, CLI) and the
//! worker HTTP API. They wrap domain types from `types.rs`, `job/`, and `executor/`.

use crate::job::{JobMetrics, JobState};
use crate::types::{JobAssets, JobManifest, WorkerCapabilities};
use serde::{Deserialize, Serialize};

// ============================================================================
// Job Submission
// ============================================================================

/// Request to submit a new job for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitJobRequest {
    /// Resource requirements and runtime configuration.
    pub manifest: JobManifest,
    /// Code, input data, and additional files.
    pub assets: JobAssets,
    /// Optional encrypted payment ticket (for future billing integration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_ticket: Option<Vec<u8>>,
}

/// Response after submitting a job (HTTP 202 Accepted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitJobResponse {
    /// Unique job identifier assigned by the worker.
    pub job_id: String,
    /// Initial job status (typically "accepted").
    pub status: JobState,
}

// ============================================================================
// Job Status
// ============================================================================

/// Response for job status queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusResponse {
    /// Job identifier.
    pub job_id: String,
    /// Current job state.
    pub state: JobState,
    /// Timing metrics (populated as job progresses).
    pub metrics: JobMetrics,
    /// Exit code (set when execution completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

// ============================================================================
// Job Result
// ============================================================================

/// Response containing job execution results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultResponse {
    /// Job identifier.
    pub job_id: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Captured stdout (base64 when binary).
    pub stdout: Vec<u8>,
    /// Captured stderr (base64 when binary).
    pub stderr: Vec<u8>,
    /// Result data (tarball of /output directory).
    pub result: Vec<u8>,
    /// BLAKE3 hash of result data (hex-encoded).
    pub result_hash: String,
}

// ============================================================================
// Health & Capabilities
// ============================================================================

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// "ok" or "degraded".
    pub status: String,
    /// Current worker state.
    pub worker_state: String,
    /// Number of available job slots.
    pub available_slots: u32,
    /// Server uptime in seconds.
    pub uptime_secs: u64,
}

/// Capabilities discovery response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesResponse {
    /// Worker hardware/software capabilities.
    pub capabilities: WorkerCapabilities,
    /// List of supported runtime identifiers.
    pub runtimes: Vec<String>,
}

// ============================================================================
// Error Response
// ============================================================================

/// Standard API error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Machine-readable error code (e.g., "CAPACITY_FULL", "NOT_FOUND").
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(id: &str) -> Self {
        Self::new("NOT_FOUND", format!("Job not found: {}", id))
    }

    pub fn capacity_full() -> Self {
        Self::new("CAPACITY_FULL", "Worker at capacity, no available slots")
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", msg)
    }

    pub fn not_ready() -> Self {
        Self::new("NOT_READY", "Result not yet available")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JobAssets, JobManifest};

    #[test]
    fn test_submit_job_request_serde() {
        let req = SubmitJobRequest {
            manifest: JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                runtime: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: Default::default(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            assets: JobAssets::inline(b"print('hi')".to_vec(), None),
            encrypted_ticket: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: SubmitJobRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.manifest.vcpu, 1);
    }

    #[test]
    fn test_api_error_constructors() {
        let err = ApiError::not_found("job-123");
        assert_eq!(err.code, "NOT_FOUND");
        assert!(err.message.contains("job-123"));

        let err = ApiError::capacity_full();
        assert_eq!(err.code, "CAPACITY_FULL");
    }

    #[test]
    fn test_health_response_serde() {
        let resp = HealthResponse {
            status: "ok".to_string(),
            worker_state: "online".to_string(),
            available_slots: 4,
            uptime_secs: 3600,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.available_slots, 4);
    }
}
