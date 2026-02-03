//! Management request handler
//!
//! Processes incoming management requests and generates responses.

use super::capability::{Capability, CapabilityError, CapabilityManager, Role};
use super::config::NodeConfig;
use super::protocol::{
    CapabilityInfo, ManagementRequest, ManagementResponse, MetricsSnapshot, NodeStatus, SystemInfo,
    WorkerState,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Management request handler
pub struct ManagementHandler {
    /// Current node configuration
    config: Arc<RwLock<NodeConfig>>,
    /// Capability manager for auth
    capability_manager: Arc<RwLock<CapabilityManager>>,
    /// Current worker state
    state: Arc<RwLock<WorkerState>>,
    /// Node ID (ed25519 public key hex)
    node_id: String,
    /// Start time for uptime calculation
    start_time: u64,
}

impl ManagementHandler {
    /// Create a new management handler
    pub fn new(
        config: NodeConfig,
        secret_key: [u8; 32],
        node_id: String,
    ) -> Self {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            config: Arc::new(RwLock::new(config)),
            capability_manager: Arc::new(RwLock::new(CapabilityManager::new(secret_key))),
            state: Arc::new(RwLock::new(WorkerState::Unregistered)),
            node_id,
            start_time,
        }
    }

    /// Handle a management request
    ///
    /// Validates capability token and dispatches to appropriate handler.
    pub async fn handle(
        &self,
        request: ManagementRequest,
        capability_token: &str,
    ) -> ManagementResponse {
        // Validate capability token
        let capability = match self.validate_capability(capability_token).await {
            Ok(cap) => cap,
            Err(e) => return ManagementResponse::unauthorized(e.to_string()),
        };

        // Check permissions for this request type
        let request_type = Self::request_type_name(&request);
        let required_role = Role::required_for(request_type);

        if !capability.can_perform(required_role) {
            return ManagementResponse::error(
                "FORBIDDEN",
                format!(
                    "Role '{}' cannot perform '{}' (requires '{}')",
                    capability.role, request_type, required_role
                ),
            );
        }

        // Dispatch to handler
        match request {
            // Configuration
            ManagementRequest::ApplyConfig { config } => self.handle_apply_config(config).await,
            ManagementRequest::GetConfig => self.handle_get_config().await,

            // Status
            ManagementRequest::GetStatus => self.handle_get_status().await,
            ManagementRequest::StreamLogs { follow, lines } => {
                self.handle_stream_logs(follow, lines).await
            }
            ManagementRequest::GetMetrics => self.handle_get_metrics().await,

            // Worker lifecycle
            ManagementRequest::Register { stake_amount } => {
                self.handle_register(stake_amount).await
            }
            ManagementRequest::Unregister => self.handle_unregister().await,
            ManagementRequest::Join => self.handle_join().await,
            ManagementRequest::Drain => self.handle_drain().await,
            ManagementRequest::Undrain => self.handle_undrain().await,

            // Maintenance
            ManagementRequest::Upgrade { image_url } => self.handle_upgrade(image_url).await,
            ManagementRequest::ApplyUpgrade => self.handle_apply_upgrade().await,
            ManagementRequest::Reboot => self.handle_reboot().await,

            // Capability management
            ManagementRequest::GenerateCapability { role, ttl_days } => {
                self.handle_generate_capability(role, ttl_days).await
            }
            ManagementRequest::RevokeCapability { token_prefix } => {
                self.handle_revoke_capability(token_prefix).await
            }
            ManagementRequest::ListCapabilities => self.handle_list_capabilities().await,
        }
    }

    /// Validate a capability token
    async fn validate_capability(&self, token: &str) -> Result<Capability, CapabilityError> {
        let manager = self.capability_manager.read().await;
        manager.validate(token)
    }

    /// Get the request type name for logging/auth
    fn request_type_name(request: &ManagementRequest) -> &'static str {
        match request {
            ManagementRequest::ApplyConfig { .. } => "apply_config",
            ManagementRequest::GetConfig => "get_config",
            ManagementRequest::GetStatus => "get_status",
            ManagementRequest::StreamLogs { .. } => "stream_logs",
            ManagementRequest::GetMetrics => "get_metrics",
            ManagementRequest::Register { .. } => "register",
            ManagementRequest::Unregister => "unregister",
            ManagementRequest::Join => "join",
            ManagementRequest::Drain => "drain",
            ManagementRequest::Undrain => "undrain",
            ManagementRequest::Upgrade { .. } => "upgrade",
            ManagementRequest::ApplyUpgrade => "apply_upgrade",
            ManagementRequest::Reboot => "reboot",
            ManagementRequest::GenerateCapability { .. } => "generate_capability",
            ManagementRequest::RevokeCapability { .. } => "revoke_capability",
            ManagementRequest::ListCapabilities => "list_capabilities",
        }
    }

    // === Configuration handlers ===

    async fn handle_apply_config(&self, new_config: NodeConfig) -> ManagementResponse {
        if let Err(e) = new_config.validate() {
            return ManagementResponse::invalid_request(e.to_string());
        }

        let mut config = self.config.write().await;
        *config = new_config;

        tracing::info!("Configuration applied successfully");
        ManagementResponse::Ok
    }

    async fn handle_get_config(&self) -> ManagementResponse {
        let config = self.config.read().await;
        ManagementResponse::Config(config.clone())
    }

    // === Status handlers ===

    async fn handle_get_status(&self) -> ManagementResponse {
        let state = *self.state.read().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        ManagementResponse::Status(NodeStatus {
            state,
            node_id: self.node_id.clone(),
            jobs_completed: 0, // TODO: Connect to actual metrics
            jobs_active: 0,
            stake: None, // TODO: Connect to staking module
            active_channels: Vec::new(),
            system: SystemInfo {
                os_version: env!("CARGO_PKG_VERSION").to_string(),
                node_version: env!("CARGO_PKG_VERSION").to_string(),
                vcpus: num_cpus::get() as u8,
                memory_mb: 0, // TODO: Get actual memory
                disk_usage_pct: 0,
                attestation_valid: true, // TODO: Connect to attestation module
            },
            uptime_secs: now - self.start_time,
        })
    }

    async fn handle_stream_logs(&self, _follow: bool, lines: Option<u32>) -> ManagementResponse {
        // TODO: Implement log streaming
        let lines = lines.unwrap_or(100);
        ManagementResponse::LogLines(vec![format!(
            "Log streaming not yet implemented (requested {} lines)",
            lines
        )])
    }

    async fn handle_get_metrics(&self) -> ManagementResponse {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // TODO: Connect to actual metrics collection
        ManagementResponse::Metrics(MetricsSnapshot {
            timestamp: now,
            jobs_total: 0,
            jobs_failed: 0,
            avg_job_duration_ms: 0,
            cpu_usage_pct: 0.0,
            memory_usage_pct: 0.0,
            network_bytes_in: 0,
            network_bytes_out: 0,
            earnings_micros: 0,
        })
    }

    // === Worker lifecycle handlers ===

    async fn handle_register(&self, stake_amount: u64) -> ManagementResponse {
        let mut state = self.state.write().await;

        if *state != WorkerState::Unregistered {
            return ManagementResponse::error(
                "INVALID_STATE",
                format!("Cannot register from state: {}", *state),
            );
        }

        // TODO: Implement actual on-chain registration
        tracing::info!("Registering node with stake: {}", stake_amount);
        *state = WorkerState::Registered;

        ManagementResponse::Ok
    }

    async fn handle_unregister(&self) -> ManagementResponse {
        let mut state = self.state.write().await;

        match *state {
            WorkerState::Online | WorkerState::Registered | WorkerState::Offline => {
                // TODO: Implement actual on-chain unregistration
                tracing::info!("Beginning unbonding period");
                *state = WorkerState::Unbonding;
                ManagementResponse::Ok
            }
            _ => ManagementResponse::error(
                "INVALID_STATE",
                format!("Cannot unregister from state: {}", *state),
            ),
        }
    }

    async fn handle_join(&self) -> ManagementResponse {
        let mut state = self.state.write().await;

        if *state != WorkerState::Registered && *state != WorkerState::Offline {
            return ManagementResponse::error(
                "INVALID_STATE",
                format!("Cannot join from state: {}", *state),
            );
        }

        tracing::info!("Node joining network");
        *state = WorkerState::Online;

        ManagementResponse::Ok
    }

    async fn handle_drain(&self) -> ManagementResponse {
        let mut state = self.state.write().await;

        if *state != WorkerState::Online && *state != WorkerState::Busy {
            return ManagementResponse::error(
                "INVALID_STATE",
                format!("Cannot drain from state: {}", *state),
            );
        }

        tracing::info!("Node entering drain mode");
        *state = WorkerState::Draining;

        ManagementResponse::Ok
    }

    async fn handle_undrain(&self) -> ManagementResponse {
        let mut state = self.state.write().await;

        if *state != WorkerState::Draining {
            return ManagementResponse::error(
                "INVALID_STATE",
                format!("Cannot undrain from state: {}", *state),
            );
        }

        tracing::info!("Node exiting drain mode");
        *state = WorkerState::Online;

        ManagementResponse::Ok
    }

    // === Maintenance handlers ===

    async fn handle_upgrade(&self, image_url: String) -> ManagementResponse {
        // TODO: Implement OS image download and staging
        tracing::info!("Downloading upgrade from: {}", image_url);
        ManagementResponse::error("NOT_IMPLEMENTED", "OS upgrade not yet implemented")
    }

    async fn handle_apply_upgrade(&self) -> ManagementResponse {
        // TODO: Implement staged upgrade application
        ManagementResponse::error("NOT_IMPLEMENTED", "OS upgrade not yet implemented")
    }

    async fn handle_reboot(&self) -> ManagementResponse {
        // TODO: Implement graceful reboot
        tracing::warn!("Reboot requested");
        ManagementResponse::error("NOT_IMPLEMENTED", "Reboot not yet implemented")
    }

    // === Capability handlers ===

    async fn handle_generate_capability(
        &self,
        role: Role,
        ttl_days: Option<u32>,
    ) -> ManagementResponse {
        let manager = self.capability_manager.read().await;
        let cap = manager.generate(role, ttl_days);

        tracing::info!("Generated {} capability: {}...", role, &cap.prefix);
        ManagementResponse::Capability(cap.token)
    }

    async fn handle_revoke_capability(&self, token_prefix: String) -> ManagementResponse {
        let mut manager = self.capability_manager.write().await;
        manager.revoke(&token_prefix);

        tracing::info!("Revoked capability: {}...", &token_prefix);
        ManagementResponse::Ok
    }

    async fn handle_list_capabilities(&self) -> ManagementResponse {
        // Note: We can't list all issued capabilities, only revoked ones
        // This is because we don't store issued tokens
        let manager = self.capability_manager.read().await;
        let revoked = manager.list_revoked();

        let caps: Vec<CapabilityInfo> = revoked
            .into_iter()
            .map(|(prefix, revoked_at)| CapabilityInfo {
                prefix: prefix.to_string(),
                role: Role::Admin, // Unknown role for revoked
                created_at: 0,     // Unknown
                expires_at: Some(revoked_at), // Use revoked_at as marker
            })
            .collect();

        ManagementResponse::Capabilities(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handler() -> ManagementHandler {
        ManagementHandler::new(
            NodeConfig::default(),
            [42u8; 32],
            "test-node-id".to_string(),
        )
    }

    async fn get_admin_token(handler: &ManagementHandler) -> String {
        let manager = handler.capability_manager.read().await;
        manager.generate(Role::Admin, None).token
    }

    #[tokio::test]
    async fn test_get_status() {
        let handler = test_handler();
        let token = get_admin_token(&handler).await;

        let response = handler.handle(ManagementRequest::GetStatus, &token).await;

        match response {
            ManagementResponse::Status(status) => {
                assert_eq!(status.state, WorkerState::Unregistered);
                assert_eq!(status.node_id, "test-node-id");
            }
            _ => panic!("Expected Status response"),
        }
    }

    #[tokio::test]
    async fn test_config_roundtrip() {
        let handler = test_handler();
        let token = get_admin_token(&handler).await;

        // Get current config
        let response = handler.handle(ManagementRequest::GetConfig, &token).await;
        let config = match response {
            ManagementResponse::Config(c) => c,
            _ => panic!("Expected Config response"),
        };

        // Apply modified config
        let mut new_config = config;
        new_config.resources.max_vcpu = 16;

        let response = handler
            .handle(
                ManagementRequest::ApplyConfig { config: new_config },
                &token,
            )
            .await;
        assert!(matches!(response, ManagementResponse::Ok));

        // Verify change persisted
        let response = handler.handle(ManagementRequest::GetConfig, &token).await;
        match response {
            ManagementResponse::Config(c) => {
                assert_eq!(c.resources.max_vcpu, 16);
            }
            _ => panic!("Expected Config response"),
        }
    }

    #[tokio::test]
    async fn test_unauthorized() {
        let handler = test_handler();

        let response = handler
            .handle(ManagementRequest::GetStatus, "invalid-token")
            .await;

        match response {
            ManagementResponse::Error { code, .. } => {
                assert_eq!(code, "UNAUTHORIZED");
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_worker_lifecycle() {
        let handler = test_handler();
        let token = get_admin_token(&handler).await;

        // Initial state: Unregistered
        let status = match handler.handle(ManagementRequest::GetStatus, &token).await {
            ManagementResponse::Status(s) => s,
            _ => panic!("Expected Status"),
        };
        assert_eq!(status.state, WorkerState::Unregistered);

        // Register
        handler
            .handle(ManagementRequest::Register { stake_amount: 100 }, &token)
            .await;

        let status = match handler.handle(ManagementRequest::GetStatus, &token).await {
            ManagementResponse::Status(s) => s,
            _ => panic!("Expected Status"),
        };
        assert_eq!(status.state, WorkerState::Registered);

        // Join
        handler.handle(ManagementRequest::Join, &token).await;

        let status = match handler.handle(ManagementRequest::GetStatus, &token).await {
            ManagementResponse::Status(s) => s,
            _ => panic!("Expected Status"),
        };
        assert_eq!(status.state, WorkerState::Online);

        // Drain
        handler.handle(ManagementRequest::Drain, &token).await;

        let status = match handler.handle(ManagementRequest::GetStatus, &token).await {
            ManagementResponse::Status(s) => s,
            _ => panic!("Expected Status"),
        };
        assert_eq!(status.state, WorkerState::Draining);
    }
}
