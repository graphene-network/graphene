//! Gossip message types for the Graphene network.

use iroh::PublicKey;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerCapabilities {
    /// Maximum vCPUs available.
    pub max_vcpu: u8,

    /// Maximum memory in MB.
    pub max_memory_mb: u32,

    /// Supported unikernel images (e.g., "node-20-unikraft", "python-3.11-unikraft").
    pub kernels: Vec<String>,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            max_vcpu: 1,
            max_memory_mb: 512,
            kernels: Vec::new(),
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
}

impl Default for WorkerPricing {
    fn default() -> Self {
        Self {
            cpu_ms_micros: 1,
            memory_mb_ms_micros: 0.1,
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

    /// Required kernel.
    pub required_kernel: String,

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

    /// Kernel specification (e.g., "python:3.12", "node:20").
    pub kernel_spec: String,
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

    /// Required unikernel image.
    pub kernel: String,

    /// Allowed egress endpoints (for firewall configuration).
    pub egress_allowlist: Vec<EgressRule>,
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

/// Payment ticket for job authorization.
///
/// Contains a signed authorization from the user's payment channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTicket {
    /// Payment channel address (Solana pubkey).
    pub channel: [u8; 32],

    /// Amount authorized for this job (in lamports or tokens).
    pub amount: u64,

    /// Sequence number (prevents replay).
    pub sequence: u64,

    /// Expiry timestamp (Unix epoch seconds).
    pub expiry: u64,

    /// Ed25519 signature over (channel, amount, sequence, expiry, job_id).
    pub signature: Signature64,
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
    Request(EncryptedJobRequest),

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
