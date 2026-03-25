//! Management API types and handlers.
//!
//! These types replace the deleted `graphene_node::management` module.
//! The CLI imports these types for request/response serialization.

use crate::http::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Management Request/Response Enums (used by CLI)
// ============================================================================

/// Management operations that can be requested via the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementRequest {
    /// Get node status.
    GetStatus,
    /// Get node configuration.
    GetConfig,
    /// Apply new configuration.
    ApplyConfig { config: Box<NodeConfig> },
    /// Get metrics snapshot.
    GetMetrics,
    /// Stream log lines.
    StreamLogs { follow: bool, lines: Option<u32> },
    /// Register worker (start accepting jobs).
    Register { stake_amount: u64 },
    /// Unregister worker.
    Unregister,
    /// Join the network.
    Join,
    /// Enter drain mode.
    Drain,
    /// Exit drain mode.
    Undrain,
    /// Upgrade node software.
    Upgrade { image_url: String },
    /// Apply staged upgrade.
    ApplyUpgrade,
    /// Reboot the node.
    Reboot,
    /// Generate a capability token.
    GenerateCapability { role: Role, ttl_days: Option<u32> },
    /// Revoke a capability token.
    RevokeCapability { token_prefix: String },
    /// List active capabilities.
    ListCapabilities,
}

/// Management operation responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementResponse {
    /// Node status.
    Status(NodeStatus),
    /// Node configuration.
    Config(NodeConfig),
    /// Metrics snapshot.
    Metrics(MetricsSnapshot),
    /// Log lines.
    LogLines(Vec<String>),
    /// Capability token string.
    Capability(String),
    /// List of capabilities.
    Capabilities(Vec<CapabilityInfo>),
    /// Simple acknowledgement.
    Ok,
    /// Error response.
    Error { code: String, message: String },
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Capability roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Reader,
}

/// Node status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    /// Node identifier.
    pub node_id: String,
    /// Current worker state.
    pub state: String,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Number of active jobs.
    pub jobs_active: u32,
    /// Number of completed jobs.
    pub jobs_completed: u64,
    /// Stake information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stake: Option<StakeInfo>,
    /// Active payment channels.
    #[serde(default)]
    pub active_channels: Vec<String>,
    /// System information.
    pub system: SystemInfo,
}

/// Stake information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeInfo {
    pub amount: u64,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unbonds_at: Option<u64>,
}

/// System information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub node_version: String,
    pub os_version: String,
    pub vcpus: u32,
    pub memory_mb: u32,
    pub disk_usage_pct: f64,
    pub attestation_valid: bool,
}

/// Node configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    /// Maximum concurrent job slots.
    #[serde(default)]
    pub max_slots: Option<u32>,
    /// Supported runtimes.
    #[serde(default)]
    pub runtimes: Vec<String>,
    /// Listen address for HTTP API.
    #[serde(default)]
    pub listen_addr: Option<String>,
}

impl NodeConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(slots) = self.max_slots {
            if slots == 0 {
                return Err("max_slots must be > 0".to_string());
            }
        }
        Ok(())
    }
}

/// Metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: u64,
    pub jobs_total: u64,
    pub jobs_failed: u64,
    pub avg_job_duration_ms: u64,
    pub cpu_usage_pct: f64,
    pub memory_usage_pct: f64,
    pub network_bytes_in: u64,
    pub network_bytes_out: u64,
    pub earnings_micros: u64,
}

/// Capability token information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInfo {
    pub prefix: String,
    pub role: Role,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

/// Re-export types under `protocol` for backward compatibility with CLI imports.
pub mod protocol {
    pub use super::{CapabilityInfo, MetricsSnapshot, NodeStatus};
}

// ============================================================================
// Management Handlers
// ============================================================================

/// GET /v1/management/status
pub async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let worker_state = state.worker.state();
    let jobs = state.jobs.read().await;
    let active = jobs.values().filter(|j| !j.is_terminal()).count() as u32;
    let completed = jobs.values().filter(|j| j.is_terminal()).count() as u64;

    Json(ManagementResponse::Status(NodeStatus {
        node_id: "local".to_string(),
        state: worker_state.to_string(),
        uptime_secs: state.uptime_secs(),
        jobs_active: active,
        jobs_completed: completed,
        stake: None,
        active_channels: vec![],
        system: SystemInfo {
            node_version: env!("CARGO_PKG_VERSION").to_string(),
            os_version: std::env::consts::OS.to_string(),
            vcpus: num_cpus::get() as u32,
            memory_mb: state.capabilities.max_memory_mb,
            disk_usage_pct: 0.0,
            attestation_valid: false,
        },
    }))
}

/// GET /v1/management/metrics
pub async fn get_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let jobs = state.jobs.read().await;
    let total = jobs.len() as u64;
    let failed = jobs
        .values()
        .filter(|j| j.state == crate::job::JobState::Failed)
        .count() as u64;

    let avg_duration = if total > 0 {
        let total_ms: u64 = jobs
            .values()
            .filter_map(|j| j.compute_metrics().total_ms)
            .sum();
        total_ms / total.max(1)
    } else {
        0
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Json(ManagementResponse::Metrics(MetricsSnapshot {
        timestamp: now,
        jobs_total: total,
        jobs_failed: failed,
        avg_job_duration_ms: avg_duration,
        cpu_usage_pct: 0.0,
        memory_usage_pct: 0.0,
        network_bytes_in: 0,
        network_bytes_out: 0,
        earnings_micros: 0,
    }))
}

/// GET /v1/management/config
pub async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(ManagementResponse::Config(NodeConfig {
        max_slots: Some(state.worker.max_slots()),
        runtimes: state.capabilities.kernels.clone(),
        listen_addr: None,
    }))
}

/// POST /v1/management/config
pub async fn apply_config(
    State(_state): State<Arc<AppState>>,
    Json(_config): Json<NodeConfig>,
) -> impl IntoResponse {
    // TODO(#200): Implement dynamic config updates
    (StatusCode::OK, Json(ManagementResponse::Ok))
}

/// POST /v1/management/lifecycle/:action
pub async fn lifecycle_action(
    State(state): State<Arc<AppState>>,
    Path(action): Path<String>,
) -> impl IntoResponse {
    use crate::worker::WorkerEvent;

    let result = match action.as_str() {
        "register" => state.worker.transition(WorkerEvent::StakeConfirmed),
        "join" => state.worker.transition(WorkerEvent::JoinedGossip),
        "drain" => state.worker.transition(WorkerEvent::ShutdownRequested),
        "undrain" => state.worker.transition(WorkerEvent::Reconnected),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ManagementResponse::Error {
                    code: "INVALID_ACTION".to_string(),
                    message: format!("Unknown lifecycle action: {}", action),
                }),
            );
        }
    };

    match result {
        Ok(_new_state) => (StatusCode::OK, Json(ManagementResponse::Ok)),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(ManagementResponse::Error {
                code: "INVALID_STATE".to_string(),
                message: e.to_string(),
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_management_request_serde() {
        let req = ManagementRequest::GetStatus;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ManagementRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ManagementRequest::GetStatus));
    }

    #[test]
    fn test_management_response_ok_serde() {
        let resp = ManagementResponse::Ok;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ok"));
    }

    #[test]
    fn test_node_config_validate() {
        let config = NodeConfig::default();
        assert!(config.validate().is_ok());

        let bad = NodeConfig {
            max_slots: Some(0),
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn test_role_serde() {
        let role = Role::Admin;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"admin\"");
    }

    #[test]
    fn test_node_status_serde() {
        let status = NodeStatus {
            node_id: "test".to_string(),
            state: "online".to_string(),
            uptime_secs: 3600,
            jobs_active: 2,
            jobs_completed: 100,
            stake: None,
            active_channels: vec![],
            system: SystemInfo {
                node_version: "0.1.0".to_string(),
                os_version: "linux".to_string(),
                vcpus: 4,
                memory_mb: 8192,
                disk_usage_pct: 30.5,
                attestation_valid: true,
            },
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: NodeStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.node_id, "test");
    }
}
