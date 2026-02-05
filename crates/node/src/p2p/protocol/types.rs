//! Protocol message types for job submission.
//!
//! These types are designed for bincode serialization over QUIC streams.

use crate::p2p::messages::{JobManifest, ResultDeliveryMode, Signature64};
use crate::ticket::PaymentTicket;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Default threshold for inline code (4 MB).
/// Code larger than this will use blob mode in auto selection.
pub const INLINE_CODE_THRESHOLD: usize = 4 * 1024 * 1024;

/// Default threshold for inline input (8 MB).
/// Input larger than this will use blob mode in auto selection.
pub const INLINE_INPUT_THRESHOLD: usize = 8 * 1024 * 1024;

/// Maximum message size for the wire protocol (16 MB).
/// Inline assets must fit within this limit when combined.
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Compression algorithm used for job assets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    /// No compression applied.
    #[default]
    None,
    /// Zstandard compression.
    Zstd,
}

/// How a single asset is delivered (inline bytes or blob reference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetData {
    /// Asset data is included inline (encrypted bytes).
    Inline {
        /// The encrypted asset data.
        data: Vec<u8>,
    },
    /// Asset data is referenced by blob hash.
    Blob {
        /// BLAKE3 hash of the encrypted blob.
        hash: Hash,
        /// Optional URL to fetch the blob from (fallback if not in Iroh).
        url: Option<String>,
    },
}

impl AssetData {
    /// Creates an inline asset from encrypted data.
    pub fn inline(data: Vec<u8>) -> Self {
        Self::Inline { data }
    }

    /// Creates a blob reference asset.
    pub fn blob(hash: Hash, url: Option<String>) -> Self {
        Self::Blob { hash, url }
    }

    /// Returns true if this is an inline asset.
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// Returns true if this is a blob reference.
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob { .. })
    }

    /// Returns the size of the asset data in bytes.
    /// For inline assets, returns the data length.
    /// For blob assets, returns 0 (size is unknown without fetching).
    pub fn inline_size(&self) -> usize {
        match self {
            Self::Inline { data } => data.len(),
            Self::Blob { .. } => 0,
        }
    }

    /// Returns the blob hash if this is a blob reference.
    pub fn blob_hash(&self) -> Option<&Hash> {
        match self {
            Self::Blob { hash, .. } => Some(hash),
            Self::Inline { .. } => None,
        }
    }
}

/// A file to be made available in the unikernel filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFile {
    /// Destination path in the unikernel filesystem (e.g., "/data/model.bin").
    pub path: String,
    /// The file data (inline or blob reference).
    pub data: AssetData,
}

impl JobFile {
    /// Creates a new job file with inline data.
    pub fn inline(path: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            data: AssetData::inline(data),
        }
    }

    /// Creates a new job file with a blob reference.
    pub fn blob(path: impl Into<String>, hash: Hash, url: Option<String>) -> Self {
        Self {
            path: path.into(),
            data: AssetData::blob(hash, url),
        }
    }
}

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

/// Code, input, and additional files for a job.
///
/// Assets can be delivered inline (embedded in the request) or via blob references.
/// Inline delivery is faster for small payloads, while blob delivery is better for
/// large files or when pre-staging/deduplication is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAssets {
    /// The code to execute (required).
    pub code: AssetData,

    /// Optional input data.
    #[serde(default)]
    pub input: Option<AssetData>,

    /// Additional files to make available in the unikernel filesystem.
    #[serde(default)]
    pub files: Vec<JobFile>,

    /// Compression algorithm applied to all assets (before encryption).
    #[serde(default)]
    pub compression: Compression,
}

impl JobAssets {
    /// Creates a new JobAssets with blob references (legacy format).
    ///
    /// This is provided for backward compatibility with existing code.
    pub fn from_blobs(
        code_hash: Hash,
        code_url: Option<String>,
        input_hash: Hash,
        input_url: Option<String>,
    ) -> Self {
        let input = if input_hash.as_bytes().iter().all(|&b| b == 0) {
            None
        } else {
            Some(AssetData::blob(input_hash, input_url))
        };

        Self {
            code: AssetData::blob(code_hash, code_url),
            input,
            files: Vec::new(),
            compression: Compression::None,
        }
    }

    /// Creates a new JobAssets with inline code and optional input.
    pub fn inline(code: Vec<u8>, input: Option<Vec<u8>>) -> Self {
        Self {
            code: AssetData::inline(code),
            input: input.map(AssetData::inline),
            files: Vec::new(),
            compression: Compression::None,
        }
    }

    /// Creates a new JobAssets with blob references.
    pub fn blobs(code_hash: Hash, input_hash: Option<Hash>) -> Self {
        Self {
            code: AssetData::blob(code_hash, None),
            input: input_hash.map(|h| AssetData::blob(h, None)),
            files: Vec::new(),
            compression: Compression::None,
        }
    }

    /// Sets the compression algorithm.
    pub fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    /// Adds additional files to the job.
    pub fn with_files(mut self, files: Vec<JobFile>) -> Self {
        self.files = files;
        self
    }

    /// Returns the total inline size of all assets.
    pub fn total_inline_size(&self) -> usize {
        let code_size = self.code.inline_size();
        let input_size = self.input.as_ref().map(|a| a.inline_size()).unwrap_or(0);
        let files_size: usize = self.files.iter().map(|f| f.data.inline_size()).sum();
        code_size + input_size + files_size
    }

    /// Returns true if all assets are inline.
    pub fn is_all_inline(&self) -> bool {
        self.code.is_inline()
            && self.input.as_ref().map(|a| a.is_inline()).unwrap_or(true)
            && self.files.iter().all(|f| f.data.is_inline())
    }

    /// Returns true if all assets are blob references.
    pub fn is_all_blob(&self) -> bool {
        self.code.is_blob()
            && self.input.as_ref().map(|a| a.is_blob()).unwrap_or(true)
            && self.files.iter().all(|f| f.data.is_blob())
    }

    // Legacy accessor methods for backward compatibility

    /// Returns the code blob hash if code is a blob reference.
    ///
    /// This is a compatibility method for existing code that expects blob hashes.
    pub fn code_hash(&self) -> Option<&Hash> {
        self.code.blob_hash()
    }

    /// Returns the input blob hash if input is a blob reference.
    ///
    /// This is a compatibility method for existing code that expects blob hashes.
    pub fn input_hash(&self) -> Option<&Hash> {
        self.input.as_ref().and_then(|a| a.blob_hash())
    }
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

    /// Payment ticket does not authorize enough funds for estimated max cost.
    InsufficientPayment,

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

    /// Inline asset exceeds maximum allowed size.
    InlineTooLarge,

    /// Generic internal error.
    InternalError,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::TicketInvalid => write!(f, "payment ticket invalid"),
            RejectReason::ChannelExhausted => write!(f, "payment channel exhausted"),
            RejectReason::InsufficientPayment => write!(f, "insufficient payment for job cost"),
            RejectReason::CapacityFull => write!(f, "worker at capacity"),
            RejectReason::UnsupportedKernel => write!(f, "unsupported kernel"),
            RejectReason::ResourcesExceedLimits => write!(f, "resources exceed limits"),
            RejectReason::EnvTooLarge => write!(f, "environment variables too large"),
            RejectReason::InvalidEnvName => write!(f, "invalid environment variable name"),
            RejectReason::ReservedEnvPrefix => write!(f, "reserved GRAPHENE_* prefix"),
            RejectReason::AssetUnavailable => write!(f, "code or input unavailable"),
            RejectReason::InlineTooLarge => write!(f, "inline asset exceeds size limit"),
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

    /// Inline encrypted result payload (sync delivery only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_result: Option<Vec<u8>>,
}

/// Resource usage metrics for a completed job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,

    /// Total CPU time in milliseconds.
    pub cpu_time_ms: u64,

    /// Total network bytes received (ingress: external -> VM).
    #[serde(default)]
    pub network_rx_bytes: u64,

    /// Total network bytes transmitted (egress: VM -> external).
    #[serde(default)]
    pub network_tx_bytes: u64,

    /// Total cost charged in microtokens.
    #[serde(default)]
    pub total_cost_micros: u64,

    /// CPU cost component in microtokens.
    #[serde(default)]
    pub cpu_cost_micros: u64,

    /// Memory cost component in microtokens.
    #[serde(default)]
    pub memory_cost_micros: u64,

    /// Egress cost component in microtokens (VM -> external).
    #[serde(default)]
    pub egress_cost_micros: u64,

    /// Ingress cost component in microtokens (external -> VM).
    #[serde(default)]
    pub ingress_cost_micros: u64,
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
        let assets = JobAssets::blobs(
            Hash::from_bytes([1u8; 32]),
            Some(Hash::from_bytes([2u8; 32])),
        );
        let encoded = bincode::serialize(&assets).expect("assets serialize failed");
        let _decoded: JobAssets =
            bincode::deserialize(&encoded).expect("assets deserialize failed");
    }

    #[test]
    fn test_job_assets_inline() {
        let assets = JobAssets::inline(b"print('hello')".to_vec(), Some(b"input data".to_vec()));
        assert!(assets.code.is_inline());
        assert!(assets.input.as_ref().unwrap().is_inline());
        assert!(assets.is_all_inline());
        assert!(!assets.is_all_blob());
    }

    #[test]
    fn test_job_assets_total_inline_size() {
        let assets = JobAssets::inline(vec![0u8; 100], Some(vec![0u8; 50]));
        assert_eq!(assets.total_inline_size(), 150);
    }

    #[test]
    fn test_asset_data_helpers() {
        let inline = AssetData::inline(b"code".to_vec());
        assert!(inline.is_inline());
        assert!(!inline.is_blob());
        assert_eq!(inline.inline_size(), 4);
        assert!(inline.blob_hash().is_none());

        let blob = AssetData::blob(Hash::from_bytes([1u8; 32]), None);
        assert!(!blob.is_inline());
        assert!(blob.is_blob());
        assert_eq!(blob.inline_size(), 0);
        assert!(blob.blob_hash().is_some());
    }

    #[test]
    fn test_compression_default() {
        assert_eq!(Compression::default(), Compression::None);
    }

    #[test]
    fn test_job_file() {
        let file = JobFile::inline("/data/model.bin", vec![1, 2, 3, 4]);
        assert_eq!(file.path, "/data/model.bin");
        assert!(file.data.is_inline());
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
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
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
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            ticket: crate::ticket::PaymentTicket::new(
                [1u8; 32], 1_000_000, 1, 1700000000, [0u8; 64],
            ),
            assets: JobAssets::blobs(Hash::from_bytes([0u8; 32]), None),
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
