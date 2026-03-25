//! Core types for the OpenCapsule compute engine.
//!
//! These types define the shared vocabulary for job execution, worker capabilities,
//! and asset delivery. Previously distributed across P2P protocol modules, they are
//! now centralized here as the project moves to a local-first HTTP API model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Job Manifest & Egress
// ============================================================================

/// Resource requirements for a job (plaintext - worker needs for allocation).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobManifest {
    /// Required vCPUs.
    pub vcpu: u8,

    /// Required memory in MB.
    pub memory_mb: u32,

    /// Maximum execution time in milliseconds.
    pub timeout_ms: u64,

    /// Required unikernel runtime.
    pub runtime: String,

    /// Allowed egress endpoints (for firewall configuration).
    pub egress_allowlist: Vec<EgressRule>,

    /// Environment variables to set in the unikernel.
    /// Names must match `^[A-Za-z_][A-Za-z0-9_]*$` and cannot use `OPENCAPSULE_*` prefix.
    /// Total size (keys + values) must not exceed 128KB.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Estimated network egress in megabytes (VM -> external).
    #[serde(default)]
    pub estimated_egress_mb: Option<u64>,

    /// Estimated network ingress in megabytes (external -> VM).
    #[serde(default)]
    pub estimated_ingress_mb: Option<u64>,
}

/// An allowed egress destination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EgressRule {
    /// Hostname or IP address.
    pub host: String,

    /// Port number.
    pub port: u16,

    /// Protocol (tcp/udp).
    pub protocol: String,
}

// ============================================================================
// Worker Capabilities & Load
// ============================================================================

/// Type of disk storage available on a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiskType {
    Ssd,
    Nvme,
    Hdd,
}

/// Disk storage capability of a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct DiskCapability {
    pub max_disk_gb: u32,
    pub disk_type: DiskType,
}

/// GPU capability of a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct GpuCapability {
    pub model: String,
    pub vram_mb: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_capability: Option<String>,
}

/// Worker hardware and software capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerCapabilities {
    /// Maximum vCPUs available.
    pub max_vcpu: u8,
    /// Maximum memory in MB.
    pub max_memory_mb: u32,
    /// Supported unikernel images (e.g., "node-20-unikraft", "python-3.11-unikraft").
    pub kernels: Vec<String>,
    /// Disk storage capability.
    pub disk: Option<DiskCapability>,
    /// GPU capabilities (empty if no GPUs available).
    pub gpus: Vec<GpuCapability>,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            max_vcpu: 1,
            max_memory_mb: 512,
            kernels: Vec::new(),
            disk: None,
            gpus: Vec::new(),
        }
    }
}

/// Worker load status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerLoad {
    /// Number of available job slots.
    pub available_slots: u8,
    /// Number of jobs waiting in queue.
    pub queue_depth: u32,
}

impl Default for WorkerLoad {
    fn default() -> Self {
        Self {
            available_slots: 1,
            queue_depth: 0,
        }
    }
}

// ============================================================================
// Job Assets & Compression
// ============================================================================

/// Compression algorithm used for job assets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    #[default]
    None,
    Zstd,
}

/// How a single asset is delivered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetData {
    /// Asset data is included inline.
    Inline {
        data: Vec<u8>,
    },
    /// Asset data is referenced by hash.
    Hash {
        /// BLAKE3 hash of the data.
        hash: [u8; 32],
        /// Optional URL to fetch the data from.
        url: Option<String>,
    },
}

impl AssetData {
    /// Creates an inline asset.
    pub fn inline(data: Vec<u8>) -> Self {
        Self::Inline { data }
    }

    /// Creates a hash reference asset.
    pub fn hash_ref(hash: [u8; 32], url: Option<String>) -> Self {
        Self::Hash { hash, url }
    }

    /// Returns true if this is an inline asset.
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// Returns the size of inline data, or 0 for hash refs.
    pub fn inline_size(&self) -> usize {
        match self {
            Self::Inline { data } => data.len(),
            Self::Hash { .. } => 0,
        }
    }
}

/// A file to be made available in the unikernel filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFile {
    /// Destination path in the unikernel filesystem.
    pub path: String,
    /// The file data.
    pub data: AssetData,
}

impl JobFile {
    pub fn inline(path: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            data: AssetData::inline(data),
        }
    }
}

/// Code, input, and additional files for a job.
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
    /// Compression algorithm applied to all assets.
    #[serde(default)]
    pub compression: Compression,
}

impl JobAssets {
    /// Creates assets with inline code and optional input.
    pub fn inline(code: Vec<u8>, input: Option<Vec<u8>>) -> Self {
        Self {
            code: AssetData::inline(code),
            input: input.map(AssetData::inline),
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
}

// ============================================================================
// Job Status
// ============================================================================

/// Status of a job in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Timeout,
    Rejected(RejectReason),
}

impl JobStatus {
    pub fn is_rejected(&self) -> bool {
        matches!(self, JobStatus::Rejected(_))
    }

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
    CapacityFull,
    UnsupportedRuntime,
    ResourcesExceedLimits,
    EnvTooLarge,
    InvalidEnvName,
    ReservedEnvPrefix,
    AssetUnavailable,
    InlineTooLarge,
    InternalError,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::CapacityFull => write!(f, "worker at capacity"),
            RejectReason::UnsupportedRuntime => write!(f, "unsupported kernel"),
            RejectReason::ResourcesExceedLimits => write!(f, "resources exceed limits"),
            RejectReason::EnvTooLarge => write!(f, "environment variables too large"),
            RejectReason::InvalidEnvName => write!(f, "invalid environment variable name"),
            RejectReason::ReservedEnvPrefix => write!(f, "reserved OPENCAPSULE_* prefix"),
            RejectReason::AssetUnavailable => write!(f, "code or input unavailable"),
            RejectReason::InlineTooLarge => write!(f, "inline asset exceeds size limit"),
            RejectReason::InternalError => write!(f, "internal error"),
        }
    }
}

// ============================================================================
// Job Metrics
// ============================================================================

/// Resource usage metrics for a completed job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceMetrics {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Total CPU time in milliseconds.
    pub cpu_time_ms: u64,
    /// Total network bytes received (ingress).
    #[serde(default)]
    pub network_rx_bytes: u64,
    /// Total network bytes transmitted (egress).
    #[serde(default)]
    pub network_tx_bytes: u64,
}

// ============================================================================
// Progress Updates
// ============================================================================

/// Kind of progress update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    Queued,
    FetchingAssets,
    Building,
    CacheHit,
    Starting,
    Running,
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
        assert!(JobStatus::Rejected(RejectReason::CapacityFull).is_terminal());
    }

    #[test]
    fn test_reject_reason_display() {
        assert_eq!(RejectReason::CapacityFull.to_string(), "worker at capacity");
        assert_eq!(
            RejectReason::EnvTooLarge.to_string(),
            "environment variables too large"
        );
    }

    #[test]
    fn test_job_assets_inline() {
        let assets = JobAssets::inline(b"print('hello')".to_vec(), Some(b"input data".to_vec()));
        assert!(assets.code.is_inline());
        assert!(assets.input.as_ref().unwrap().is_inline());
        assert!(assets.is_all_inline());
    }

    #[test]
    fn test_job_assets_total_inline_size() {
        let assets = JobAssets::inline(vec![0u8; 100], Some(vec![0u8; 50]));
        assert_eq!(assets.total_inline_size(), 150);
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
    fn test_worker_capabilities_default() {
        let caps = WorkerCapabilities::default();
        assert_eq!(caps.max_vcpu, 1);
        assert_eq!(caps.max_memory_mb, 512);
    }

    #[test]
    fn test_worker_load_default() {
        let load = WorkerLoad::default();
        assert_eq!(load.available_slots, 1);
        assert_eq!(load.queue_depth, 0);
    }

    #[test]
    fn test_job_manifest_serde() {
        let manifest = JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30000,
            runtime: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: Default::default(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: JobManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, parsed);
    }
}
