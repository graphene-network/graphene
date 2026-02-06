//! Gossip message types for the Graphene network.

use iroh::PublicKey;
use iroh_blobs::Hash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export canonical PaymentTicket from ticket module
pub use crate::ticket::PaymentTicket;

/// A 64-byte signature with proper serde support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature64(pub [u8; 64]);

impl Signature64 {
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl From<[u8; 64]> for Signature64 {
    fn from(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl Serialize for Signature64 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Signature64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        let arr: [u8; 64] = vec.try_into().map_err(|v: Vec<u8>| {
            serde::de::Error::custom(format!("expected 64 bytes, got {}", v.len()))
        })?;
        Ok(Self(arr))
    }
}

// ============================================================================
// Disk Capability Types
// ============================================================================

/// Type of disk storage available on a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiskType {
    /// Solid State Drive
    Ssd,
    /// NVMe SSD (faster than standard SSD)
    Nvme,
    /// Hard Disk Drive
    Hdd,
}

/// Disk storage capability of a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct DiskCapability {
    /// Maximum disk space available in GB.
    pub max_disk_gb: u32,
    /// Type of disk storage.
    pub disk_type: DiskType,
}

// ============================================================================
// GPU Capability Types
// ============================================================================

/// GPU capability of a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct GpuCapability {
    /// GPU model name (e.g., "NVIDIA RTX 4090").
    pub model: String,
    /// Video RAM in MB.
    pub vram_mb: u32,
    /// CUDA compute capability version (e.g., "8.9").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_capability: Option<String>,
}

// ============================================================================
// Region Types
// ============================================================================

/// Geographic region information for a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct WorkerRegion {
    /// ISO 3166-1 alpha-2 country code (e.g., "US", "DE", "JP").
    pub country: String,
    /// Cloud provider region identifier (e.g., "us-east-1", "eu-west-2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_region: Option<String>,
}

// ============================================================================
// Reputation Types
// ============================================================================

/// Reputation metrics for a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct WorkerReputation {
    /// Total number of jobs completed successfully.
    pub jobs_completed: u64,
    /// Total number of jobs that failed.
    pub jobs_failed: u64,
    /// Success rate as a ratio (0.0 to 1.0).
    pub success_rate: f64,
    /// 50th percentile job latency in milliseconds.
    pub latency_p50_ms: u64,
    /// 95th percentile job latency in milliseconds.
    pub latency_p95_ms: u64,
    /// 99th percentile job latency in milliseconds.
    pub latency_p99_ms: u64,
    /// Uptime percentage (0.0 to 100.0).
    pub uptime_percentage: f64,
}

impl Default for WorkerReputation {
    fn default() -> Self {
        Self {
            jobs_completed: 0,
            jobs_failed: 0,
            success_rate: 1.0,
            latency_p50_ms: 0,
            latency_p95_ms: 0,
            latency_p99_ms: 0,
            uptime_percentage: 100.0,
        }
    }
}

// ============================================================================
// Compute Messages
// ============================================================================

/// Messages broadcast on the `graphene-compute-v1` topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeMessage {
    /// Worker announcing availability.
    Announcement(WorkerAnnouncement),

    /// Periodic heartbeat from a worker.
    Heartbeat(WorkerHeartbeat),

    /// Discovery query looking for workers.
    DiscoveryQuery(DiscoveryQuery),

    /// Response to a discovery query.
    DiscoveryResponse(DiscoveryResponse),
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

/// Worker pricing information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerPricing {
    /// Price per CPU-millisecond in microtokens.
    pub cpu_ms_micros: u64,

    /// Price per memory-MB-millisecond in microtokens.
    pub memory_mb_ms_micros: f64,

    /// Price per disk-GB-millisecond in microtokens (if disk is offered).
    pub disk_gb_ms_micros: Option<f64>,

    /// Price per GPU-millisecond in microtokens (if GPU is offered).
    pub gpu_ms_micros: Option<u64>,

    /// Price per megabyte of network egress in microtokens (VM -> external).
    #[serde(default)]
    pub egress_mb_micros: Option<f64>,

    /// Price per megabyte of network ingress in microtokens (external -> VM).
    #[serde(default)]
    pub ingress_mb_micros: Option<f64>,
}

impl Default for WorkerPricing {
    fn default() -> Self {
        Self {
            cpu_ms_micros: 1,
            memory_mb_ms_micros: 0.1,
            disk_gb_ms_micros: None,
            gpu_ms_micros: None,
            egress_mb_micros: None,
            ingress_mb_micros: None,
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

/// Worker lifecycle state for gossip messages.
///
/// Duplicated here to avoid circular dependency with worker module.
/// Must be kept in sync with `crate::worker::WorkerState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum GossipWorkerState {
    /// Initial state before Solana registration.
    #[default]
    Unregistered = 0,
    /// Stake confirmed on Solana, awaiting P2P gossip join.
    Registered = 1,
    /// Active and accepting jobs (has available slots).
    Online = 2,
    /// Active but at capacity (no available slots).
    Busy = 3,
    /// Graceful shutdown initiated, finishing current jobs.
    Draining = 4,
    /// Temporarily disconnected from P2P network.
    Offline = 5,
    /// Unbonding period active (14-day cooldown).
    Unbonding = 6,
    /// Terminal state, worker has exited.
    Exited = 7,
}

impl std::fmt::Display for GossipWorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GossipWorkerState::Unregistered => write!(f, "unregistered"),
            GossipWorkerState::Registered => write!(f, "registered"),
            GossipWorkerState::Online => write!(f, "online"),
            GossipWorkerState::Busy => write!(f, "busy"),
            GossipWorkerState::Draining => write!(f, "draining"),
            GossipWorkerState::Offline => write!(f, "offline"),
            GossipWorkerState::Unbonding => write!(f, "unbonding"),
            GossipWorkerState::Exited => write!(f, "exited"),
        }
    }
}

/// Worker availability announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAnnouncement {
    /// The worker's node ID.
    pub node_id: PublicKey,

    /// Worker software version.
    pub version: String,

    /// Worker hardware and software capabilities.
    pub capabilities: WorkerCapabilities,

    /// Worker pricing information.
    pub pricing: WorkerPricing,

    /// Current load status.
    pub load: WorkerLoad,

    /// Current lifecycle state.
    pub state: GossipWorkerState,

    /// Timestamp of this announcement (Unix epoch seconds).
    pub timestamp: u64,

    /// Geographic regions where this worker operates.
    pub regions: Vec<WorkerRegion>,

    /// Reputation metrics for this worker.
    pub reputation: WorkerReputation,
}

/// Periodic heartbeat to indicate worker is still alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHeartbeat {
    /// The worker's node ID.
    pub node_id: PublicKey,

    /// Current load status.
    pub load: WorkerLoad,

    /// Current lifecycle state.
    pub state: GossipWorkerState,

    /// Timestamp of this heartbeat.
    pub timestamp: u64,
}

/// Query to find workers matching criteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    /// Unique query ID for correlation.
    pub query_id: String,

    /// Required vCPUs.
    pub required_vcpu: u8,

    /// Required memory in MB.
    pub required_memory_mb: u32,

    /// Required runtime.
    pub required_runtime: String,

    /// Maximum acceptable CPU price per ms.
    pub max_price_cpu_ms: Option<u64>,
}

/// Response to a discovery query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    /// The query ID this responds to.
    pub query_id: String,

    /// The responding worker's announcement.
    pub announcement: WorkerAnnouncement,
}

/// Messages broadcast on the `graphene-tickets-v1` topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TicketMessage {
    /// A worker accepted a payment ticket (prevents double-spend).
    TicketAccepted(TicketAccepted),

    /// A ticket was rejected (already spent).
    TicketRejected(TicketRejected),
}

/// Notification that a payment ticket was accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketAccepted {
    /// Hash of the ticket (for deduplication).
    pub ticket_hash: [u8; 32],

    /// The worker that accepted the ticket.
    pub worker_id: PublicKey,

    /// Job ID associated with this ticket.
    pub job_id: String,

    /// Timestamp of acceptance.
    pub timestamp: u64,
}

/// Notification that a payment ticket was rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketRejected {
    /// Hash of the rejected ticket.
    pub ticket_hash: [u8; 32],

    /// Reason for rejection.
    pub reason: String,

    /// Timestamp of rejection.
    pub timestamp: u64,
}

// ============================================================================
// Cache Messages
// ============================================================================

/// Announcement of a cached build artifact.
///
/// Broadcast on the `graphene-cache-v1` gossip topic to advertise
/// availability of cached unikernel builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheAnnouncement {
    /// The L3 cache key (BLAKE3 hash of kernel + requirements + code).
    pub cache_key: [u8; 32],

    /// Iroh blob hash of the cached artifact.
    pub blob_hash: Hash,

    /// Size of the cached artifact in bytes.
    pub size_bytes: u64,

    /// Runtime specification (e.g., "python:3.12", "node:20").
    pub runtime_spec: String,
}

/// Messages broadcast on the `graphene-cache-v1` topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheMessage {
    /// A node announcing availability of a cached build.
    Announcement(CacheAnnouncement),

    /// Query for a specific cache key.
    Query {
        /// The cache key being queried.
        cache_key: [u8; 32],
        /// Unique query ID for correlation.
        query_id: String,
    },

    /// Response to a cache query.
    QueryResponse {
        /// The query ID this responds to.
        query_id: String,
        /// The blob hash if available.
        blob_hash: Option<Hash>,
    },
}

/// Job result metadata for blob-based result delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// The job this result is for.
    pub job_id: String,

    /// Hash of the result blob.
    pub result_hash: Hash,

    /// Size of the result in bytes.
    pub result_size: u64,

    /// Exit code of the job (0 = success).
    pub exit_code: i32,

    /// Execution time in milliseconds.
    pub execution_ms: u64,
}

// ============================================================================
// Encrypted Job Messages (Soft Confidential Computing)
// ============================================================================

/// Result delivery mode - determines how job results are returned to the user.
///
/// Sync mode (default) streams results directly over QUIC for lowest latency.
/// Async mode uploads to Iroh blob storage for offline retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultDeliveryMode {
    /// Stream result directly over QUIC connection (~10ms latency).
    /// Skips DELIVERING state, transitions directly to DELIVERED.
    #[default]
    Sync,
    /// Upload to Iroh blob store for async retrieval (24h TTL).
    /// Uses DELIVERING state with eventual consistency.
    Async,
}

impl std::fmt::Display for ResultDeliveryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResultDeliveryMode::Sync => write!(f, "sync"),
            ResultDeliveryMode::Async => write!(f, "async"),
        }
    }
}

/// Payload containing job result data - either inline or as blob references.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ResultPayload {
    /// Inline encrypted result data (for sync delivery).
    Inline {
        /// Encrypted result/return value.
        encrypted_result: Vec<u8>,
        /// Encrypted stdout output.
        encrypted_stdout: Vec<u8>,
        /// Encrypted stderr output.
        encrypted_stderr: Vec<u8>,
    },
    /// Blob hashes for async delivery via Iroh.
    Blob {
        /// Hash of the encrypted result blob in Iroh.
        encrypted_result_hash: Hash,
        /// Hash of the encrypted stdout blob in Iroh.
        encrypted_stdout_hash: Hash,
        /// Hash of the encrypted stderr blob in Iroh.
        encrypted_stderr_hash: Hash,
    },
}

impl ResultPayload {
    /// Creates an inline payload from encrypted data.
    pub fn inline(
        encrypted_result: Vec<u8>,
        encrypted_stdout: Vec<u8>,
        encrypted_stderr: Vec<u8>,
    ) -> Self {
        Self::Inline {
            encrypted_result,
            encrypted_stdout,
            encrypted_stderr,
        }
    }

    /// Creates a blob payload from Iroh hashes.
    pub fn blob(
        encrypted_result_hash: Hash,
        encrypted_stdout_hash: Hash,
        encrypted_stderr_hash: Hash,
    ) -> Self {
        Self::Blob {
            encrypted_result_hash,
            encrypted_stdout_hash,
            encrypted_stderr_hash,
        }
    }

    /// Returns true if this is an inline payload.
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// Returns true if this is a blob payload.
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob { .. })
    }
}

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
    /// Names must match `^[A-Za-z_][A-Za-z0-9_]*$` and cannot use `GRAPHENE_*` prefix.
    /// Total size (keys + values) must not exceed 128KB.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Estimated network egress in megabytes (VM -> external).
    /// Used for cost estimation when egress pricing is enabled.
    /// If not provided, assumed to be 0 (no egress).
    #[serde(default)]
    pub estimated_egress_mb: Option<u64>,

    /// Estimated network ingress in megabytes (external -> VM).
    /// Used for cost estimation when ingress pricing is enabled.
    /// If not provided, assumed to be 0 (no ingress).
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

/// Encrypted job request from user to worker.
///
/// The manifest remains plaintext so workers can validate resource availability
/// and configure networking before decrypting the actual job data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedJobRequest {
    /// Unique job identifier.
    pub job_id: String,

    /// Ephemeral X25519 public key for forward secrecy.
    /// Used by worker to derive the job decryption key.
    pub ephemeral_pubkey: [u8; 32],

    /// Hash of the encrypted input blob in Iroh.
    pub encrypted_input_hash: Hash,

    /// Hash of the encrypted code blob in Iroh.
    pub encrypted_code_hash: Hash,

    /// Plaintext manifest (worker needs for resource allocation).
    pub manifest: JobManifest,

    /// Payment ticket authorizing computation.
    pub payment_ticket: PaymentTicket,

    /// Solana PDA of the payment channel (for key derivation).
    pub channel_pda: [u8; 32],

    /// Requested result delivery mode (defaults to Sync).
    #[serde(default)]
    pub delivery_mode: ResultDeliveryMode,
}

/// Encrypted job result from worker to user.
///
/// Exit code and execution time remain plaintext for state machine handling.
/// The payload can be either inline (sync delivery) or blob references (async).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedJobResult {
    /// The job this result is for.
    pub job_id: String,

    /// Result payload - inline data or Iroh blob hashes.
    pub payload: ResultPayload,

    /// Exit code of the job (0 = success).
    pub exit_code: i32,

    /// Execution time in milliseconds.
    pub execution_ms: u64,

    /// Worker's Ed25519 signature over the result.
    pub worker_signature: Signature64,
}

/// Messages for encrypted job protocol on `graphene-jobs-v1` topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncryptedJobMessage {
    /// User submitting an encrypted job.
    Request(Box<EncryptedJobRequest>),

    /// Worker acknowledging job receipt.
    Accepted {
        job_id: String,
        worker_id: PublicKey,
        estimated_start_ms: u64,
    },

    /// Worker reporting job completion.
    Completed(EncryptedJobResult),

    /// Worker reporting job failure.
    Failed {
        job_id: String,
        reason: String,
        refund_ticket: Option<PaymentTicket>,
    },
}
