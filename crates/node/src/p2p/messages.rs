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

/// Worker availability announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAnnouncement {
    /// The worker's node ID.
    pub node_id: PublicKey,

    /// Supported capability tags (e.g., "gpu", "high-memory").
    pub capabilities: Vec<String>,

    /// Current price per compute unit (in smallest token denomination).
    pub price_per_unit: u64,

    /// Maximum job duration in seconds.
    pub max_duration_secs: u64,

    /// Timestamp of this announcement (Unix epoch seconds).
    pub timestamp: u64,
}

/// Periodic heartbeat to indicate worker is still alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHeartbeat {
    /// The worker's node ID.
    pub node_id: PublicKey,

    /// Current load (0-100 percentage).
    pub load_percent: u8,

    /// Number of active jobs.
    pub active_jobs: u32,

    /// Timestamp of this heartbeat.
    pub timestamp: u64,
}

/// Query to find workers matching criteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    /// Unique query ID for correlation.
    pub query_id: String,

    /// Required capabilities.
    pub required_capabilities: Vec<String>,

    /// Maximum acceptable price per unit.
    pub max_price: Option<u64>,
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
