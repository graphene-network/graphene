//! Management protocol request/response types
//!
//! Simple serde-based protocol over Iroh QUIC streams.

use super::config::NodeConfig;
use super::Role;
use serde::{Deserialize, Serialize};

/// Management request types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManagementRequest {
    // Configuration
    /// Apply new node configuration
    ApplyConfig { config: NodeConfig },
    /// Get current configuration
    GetConfig,

    // Status
    /// Get node status
    GetStatus,
    /// Stream logs (returns multiple responses)
    StreamLogs {
        follow: bool,
        lines: Option<u32>,
    },
    /// Get metrics snapshot
    GetMetrics,

    // Worker lifecycle (WHITEPAPER.md Section 12.4)
    /// Register node on-chain (UNREGISTERED → REGISTERED)
    Register { stake_amount: u64 },
    /// Unregister from network (begins UNBONDING)
    Unregister,
    /// Join network (REGISTERED → ONLINE)
    Join,
    /// Enter maintenance mode (ONLINE → DRAINING)
    Drain,
    /// Exit maintenance mode (DRAINING → ONLINE)
    Undrain,

    // Maintenance
    /// Download and stage OS upgrade
    Upgrade { image_url: String },
    /// Apply staged upgrade (triggers reboot)
    ApplyUpgrade,
    /// Immediate reboot
    Reboot,

    // Capability management
    /// Generate new capability token
    GenerateCapability {
        role: Role,
        ttl_days: Option<u32>,
    },
    /// Revoke capability by token prefix
    RevokeCapability { token_prefix: String },
    /// List active capabilities
    ListCapabilities,
}

/// Management response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManagementResponse {
    /// Success with no additional data
    Ok,
    /// Configuration data
    Config(NodeConfig),
    /// Node status
    Status(NodeStatus),
    /// Metrics snapshot
    Metrics(MetricsSnapshot),
    /// Capability token
    Capability(String),
    /// List of capabilities
    Capabilities(Vec<CapabilityInfo>),
    /// Log lines (for StreamLogs)
    LogLines(Vec<String>),
    /// Error response
    Error { code: String, message: String },
}

impl ManagementResponse {
    /// Create an error response
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Create an unauthorized error
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::error("UNAUTHORIZED", message)
    }

    /// Create an invalid request error
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::error("INVALID_REQUEST", message)
    }

    /// Create an internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::error("INTERNAL_ERROR", message)
    }
}

/// Node status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    /// Current worker state (from WHITEPAPER.md Section 12.4)
    pub state: WorkerState,
    /// Node public identifier
    pub node_id: String,
    /// Jobs completed since startup
    pub jobs_completed: u64,
    /// Currently active jobs
    pub jobs_active: u64,
    /// Staking information
    pub stake: Option<StakeInfo>,
    /// Active payment channels
    pub active_channels: Vec<String>,
    /// System information
    pub system: SystemInfo,
    /// Uptime in seconds
    pub uptime_secs: u64,
}

/// Worker state machine (WHITEPAPER.md Section 12.4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Not registered on-chain
    Unregistered,
    /// Registered but not accepting jobs
    Registered,
    /// Active and accepting jobs
    Online,
    /// Executing a job
    Busy,
    /// Graceful maintenance mode
    Draining,
    /// Temporarily offline
    Offline,
    /// Unbonding period (14 days)
    Unbonding,
    /// Fully exited
    Exited,
}

impl std::fmt::Display for WorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerState::Unregistered => write!(f, "UNREGISTERED"),
            WorkerState::Registered => write!(f, "REGISTERED"),
            WorkerState::Online => write!(f, "ONLINE"),
            WorkerState::Busy => write!(f, "BUSY"),
            WorkerState::Draining => write!(f, "DRAINING"),
            WorkerState::Offline => write!(f, "OFFLINE"),
            WorkerState::Unbonding => write!(f, "UNBONDING"),
            WorkerState::Exited => write!(f, "EXITED"),
        }
    }
}

/// Staking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeInfo {
    /// Amount staked in $GRAPHENE
    pub amount: u64,
    /// Staking address
    pub address: String,
    /// If unbonding, when it completes
    pub unbonds_at: Option<u64>,
}

/// System information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// OS version
    pub os_version: String,
    /// Node binary version
    pub node_version: String,
    /// Available vCPUs
    pub vcpus: u8,
    /// Available memory in MB
    pub memory_mb: u32,
    /// Disk usage percentage
    pub disk_usage_pct: u8,
    /// Platform attestation status
    pub attestation_valid: bool,
}

/// Metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Timestamp of snapshot
    pub timestamp: u64,
    /// Jobs completed total
    pub jobs_total: u64,
    /// Jobs failed total
    pub jobs_failed: u64,
    /// Average job duration in ms
    pub avg_job_duration_ms: u64,
    /// CPU usage percentage
    pub cpu_usage_pct: f32,
    /// Memory usage percentage
    pub memory_usage_pct: f32,
    /// Network bytes in
    pub network_bytes_in: u64,
    /// Network bytes out
    pub network_bytes_out: u64,
    /// Earnings in current period (micros)
    pub earnings_micros: u64,
}

/// Capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInfo {
    /// Token prefix (first 8 chars)
    pub prefix: String,
    /// Role granted
    pub role: Role,
    /// When created
    pub created_at: u64,
    /// When expires (if set)
    pub expires_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = ManagementRequest::GetStatus;
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("get_status"));
    }

    #[test]
    fn test_response_serialization() {
        let response = ManagementResponse::error("TEST", "Test error");
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("TEST"));
        assert!(json.contains("Test error"));
    }

    #[test]
    fn test_worker_state_display() {
        assert_eq!(WorkerState::Online.to_string(), "ONLINE");
        assert_eq!(WorkerState::Draining.to_string(), "DRAINING");
    }
}
