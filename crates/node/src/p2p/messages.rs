//! Gossip message types for the Graphene network.

use iroh::PublicKey;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};

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
