//! Iroh-based management client

#![allow(dead_code)]

use monad_node::management::{ManagementRequest, ManagementResponse};

/// Management client for connecting to Graphene nodes
pub struct ManagementClient {
    /// Node ID to connect to
    node_id: String,
    /// Capability token for authentication
    capability: String,
    /// Optional direct endpoint
    endpoint: Option<String>,
}

impl ManagementClient {
    /// Create a new management client
    pub fn new(node_id: String, capability: String, endpoint: Option<String>) -> Self {
        Self {
            node_id,
            capability,
            endpoint,
        }
    }

    /// Send a management request and receive response
    pub async fn request(&self, request: ManagementRequest) -> anyhow::Result<ManagementResponse> {
        // TODO: Implement actual Iroh connection
        // 1. Create Iroh endpoint
        // 2. Connect to node using node_id
        // 3. Open QUIC stream with MANAGEMENT_ALPN
        // 4. Send request as JSON
        // 5. Receive and parse response

        tracing::debug!("Would send request to node {}: {:?}", self.node_id, request);

        anyhow::bail!("Iroh client not yet implemented")
    }
}
