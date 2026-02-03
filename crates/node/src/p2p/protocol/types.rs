//! Protocol message types for job submission.
//!
//! These types are designed for bincode serialization over QUIC streams.

use crate::p2p::messages::{JobManifest, ResultDeliveryMode, Signature64};
use crate::ticket::PaymentTicket;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Job submission request from client to worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    /// Unique job identifier.
    pub job_id: Uuid,

    /// Resource requirements and configuration.
    pub manifest: JobManifest,

    /// Payment authorization ticket.
    pub ticket: PaymentTicket,

    /// Code and input blob references.
    pub assets: JobAssets,

    /// Ephemeral X25519 public key for forward secrecy.
    /// Used by worker to derive the job decryption key.
    pub ephemeral_pubkey: [u8; 32],

    /// Solana PDA of the payment channel (for key derivation).
    pub channel_pda: [u8; 32],

    /// Requested result delivery mode (defaults to Sync).
    #[serde(default)]
    pub delivery_mode: ResultDeliveryMode,
}

/// References to code and input blobs in Iroh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAssets {
    /// BLAKE3 hash of the encrypted code blob.
    pub code_hash: Hash,

    /// Optional URL to fetch code from (fallback if not in Iroh).
    pub code_url: Option<String>,

    /// BLAKE3 hash of the encrypted input blob.
    pub input_hash: Hash,

    /// Optional URL to fetch input from (fallback if not in Iroh).
    pub input_url: Option<String>,
}

/// Response to a job request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResponse {
    /// The job ID this response refers to.
    pub job_id: Uuid,

    /// Current status of the job.
    pub status: JobStatus,

    /// Job result (only present when status is Succeeded, Failed, or Timeout).
    pub result: Option<JobResult>,

    /// Error message (only present when status is Rejected).
    pub error: Option<String>,
}

/// Status of a job in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Job accepted and queued for execution.
    Accepted,

    /// Job is currently running.
    Running,

    /// Job completed successfully (exit code 0).
    Succeeded,

    /// Job failed (non-zero exit code).
    Failed,

    /// Job exceeded time limit.
    Timeout,

    /// Job was rejected (see RejectReason).
    Rejected(RejectReason),
}

impl JobStatus {
    /// Returns true if this status indicates the job was rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(self, JobStatus::Rejected(_))
    }

    /// Returns true if this is a terminal status.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Timeout | JobStatus::Rejected(_)
        )
    }
}

/// Reason for job rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// Payment ticket signature or format is invalid.
    TicketInvalid,

    /// Payment channel balance exhausted or nonce replayed.
    ChannelExhausted,

    /// Worker is at capacity (no available slots).
    CapacityFull,

    /// Requested kernel is not supported by this worker.
    UnsupportedKernel,

    /// Requested resources exceed worker limits.
    ResourcesExceedLimits,

    /// Environment variables total size exceeds limit.
    EnvTooLarge,

    /// Environment variable name is invalid.
    InvalidEnvName,

    /// Environment variable uses reserved GRAPHENE_* prefix.
    ReservedEnvPrefix,

    /// Code or input blob could not be fetched.
    AssetUnavailable,

    /// Generic internal error.
    InternalError,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::TicketInvalid => write!(f, "payment ticket invalid"),
            RejectReason::ChannelExhausted => write!(f, "payment channel exhausted"),
            RejectReason::CapacityFull => write!(f, "worker at capacity"),
            RejectReason::UnsupportedKernel => write!(f, "unsupported kernel"),
            RejectReason::ResourcesExceedLimits => write!(f, "resources exceed limits"),
            RejectReason::EnvTooLarge => write!(f, "environment variables too large"),
            RejectReason::InvalidEnvName => write!(f, "invalid environment variable name"),
            RejectReason::ReservedEnvPrefix => write!(f, "reserved GRAPHENE_* prefix"),
            RejectReason::AssetUnavailable => write!(f, "code or input unavailable"),
            RejectReason::InternalError => write!(f, "internal error"),
        }
    }
}

/// Job execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// BLAKE3 hash of the encrypted result blob.
    pub result_hash: Hash,

    /// Optional URL to fetch result from (for async delivery).
    pub result_url: Option<String>,

    /// Exit code of the unikernel (0 = success).
    pub exit_code: i32,

    /// Execution duration in milliseconds.
    pub duration_ms: u64,

    /// Resource usage metrics.
    pub metrics: JobMetrics,

    /// Worker's Ed25519 signature over the result.
    /// Signs: job_id || result_hash || exit_code || duration_ms
    pub worker_signature: Signature64,
}

/// Resource usage metrics for a completed job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,

    /// Total CPU time in milliseconds.
    pub cpu_time_ms: u64,

    /// Total network bytes received.
    #[serde(default)]
    pub network_rx_bytes: u64,

    /// Total network bytes transmitted.
    #[serde(default)]
    pub network_tx_bytes: u64,
}

/// Progress update during job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    /// The job this progress refers to.
    pub job_id: Uuid,

    /// Type of progress update.
    pub kind: ProgressKind,

    /// Progress percentage (0-100) if applicable.
    pub percent: Option<u8>,

    /// Human-readable status message.
    pub message: Option<String>,
}

/// Kind of progress update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    /// Job queued, waiting to start.
    Queued,
    /// Fetching code and input blobs.
    FetchingAssets,
    /// Building unikernel (cache miss).
    Building,
    /// Using cached unikernel (cache hit).
    CacheHit,
    /// Starting unikernel execution.
    Starting,
    /// Execution in progress.
    Running,
    /// Uploading result (async mode).
    Uploading,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_is_terminal() {
        assert!(!JobStatus::Accepted.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Succeeded.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Timeout.is_terminal());
        assert!(JobStatus::Rejected(RejectReason::TicketInvalid).is_terminal());
    }

    #[test]
    fn test_job_status_is_rejected() {
        assert!(!JobStatus::Accepted.is_rejected());
        assert!(!JobStatus::Succeeded.is_rejected());
        assert!(JobStatus::Rejected(RejectReason::CapacityFull).is_rejected());
    }

    #[test]
    fn test_reject_reason_display() {
        assert_eq!(
            RejectReason::TicketInvalid.to_string(),
            "payment ticket invalid"
        );
        assert_eq!(
            RejectReason::EnvTooLarge.to_string(),
            "environment variables too large"
        );
    }

    #[test]
    fn test_job_assets_bincode() {
        let assets = JobAssets {
            code_hash: Hash::from_bytes([1u8; 32]),
            code_url: None,
            input_hash: Hash::from_bytes([2u8; 32]),
            input_url: None,
        };
        let encoded = bincode::serialize(&assets).expect("assets serialize failed");
        let _decoded: JobAssets =
            bincode::deserialize(&encoded).expect("assets deserialize failed");
    }

    #[test]
    fn test_payment_ticket_bincode() {
        let ticket =
            crate::ticket::PaymentTicket::new([1u8; 32], 1_000_000, 1, 1700000000, [0u8; 64]);
        let encoded = bincode::serialize(&ticket).expect("ticket serialize failed");
        let _decoded: crate::ticket::PaymentTicket =
            bincode::deserialize(&encoded).expect("ticket deserialize failed");
    }

    #[test]
    fn test_job_manifest_bincode() {
        let manifest = JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: Default::default(),
        };
        let encoded = bincode::serialize(&manifest).expect("manifest serialize failed");
        let _decoded: JobManifest =
            bincode::deserialize(&encoded).expect("manifest deserialize failed");
    }

    #[test]
    fn test_job_request_bincode_roundtrip() {
        let request = JobRequest {
            job_id: Uuid::new_v4(),
            manifest: JobManifest {
                vcpu: 2,
                memory_mb: 512,
                timeout_ms: 30000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: Default::default(),
            },
            ticket: crate::ticket::PaymentTicket::new(
                [1u8; 32], 1_000_000, 1, 1700000000, [0u8; 64],
            ),
            assets: JobAssets {
                code_hash: Hash::from_bytes([0u8; 32]),
                code_url: None,
                input_hash: Hash::from_bytes([0u8; 32]),
                input_url: None,
            },
            ephemeral_pubkey: [0u8; 32],
            channel_pda: [0u8; 32],
            delivery_mode: ResultDeliveryMode::Sync,
        };

        let encoded = bincode::serialize(&request).expect("serialize failed");
        let decoded: JobRequest = bincode::deserialize(&encoded).expect("deserialize failed");

        assert_eq!(request.job_id, decoded.job_id);
        assert_eq!(request.manifest.vcpu, decoded.manifest.vcpu);
        assert_eq!(request.manifest.kernel, decoded.manifest.kernel);
    }

    #[test]
    fn test_job_response_bincode_roundtrip() {
        let response = JobResponse {
            job_id: Uuid::new_v4(),
            status: JobStatus::Rejected(RejectReason::CapacityFull),
            result: None,
            error: Some("Worker is busy".to_string()),
        };

        let encoded = bincode::serialize(&response).expect("serialize failed");
        let decoded: JobResponse = bincode::deserialize(&encoded).expect("deserialize failed");

        assert_eq!(response.job_id, decoded.job_id);
        assert_eq!(response.status, decoded.status);
        assert_eq!(response.error, decoded.error);
    }
}
