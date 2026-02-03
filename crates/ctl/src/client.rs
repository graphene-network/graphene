//! Iroh-based management client

#![allow(dead_code)]

use crate::config::NodeEntry;
use iroh::endpoint::Endpoint;
use iroh::{EndpointAddr, PublicKey};
use monad_node::management::{ManagementRequest, ManagementResponse, MANAGEMENT_ALPN};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Client errors
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Invalid node ID: {0}")]
    InvalidNodeId(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Request timeout")]
    Timeout,
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Wire protocol error: {0}")]
    WireError(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Client configuration options
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Timeout for short operations (status, config, metrics)
    pub request_timeout: Duration,
    /// Timeout for long operations (upgrade, drain, reboot)
    pub long_timeout: Duration,
    /// Number of retry attempts
    pub retry_attempts: u32,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            long_timeout: Duration::from_secs(300),
            retry_attempts: 3,
        }
    }
}

/// Authenticated request wrapper for wire protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedRequest {
    pub capability: String,
    pub request: ManagementRequest,
}

/// Management client for connecting to Graphene nodes
pub struct ManagementClient {
    node_id: PublicKey,
    capability: String,
    #[allow(dead_code)]
    endpoint_hint: Option<String>,
    options: ClientOptions,
}

impl ManagementClient {
    /// Create a client from a node entry configuration
    pub fn from_config(entry: &NodeEntry, options: ClientOptions) -> Result<Self, ClientError> {
        let node_id: PublicKey = entry
            .node_id
            .parse()
            .map_err(|e| ClientError::InvalidNodeId(format!("{}: {}", entry.node_id, e)))?;

        Ok(Self {
            node_id,
            capability: entry.capability.clone(),
            endpoint_hint: entry.endpoint.clone(),
            options,
        })
    }

    /// Create a client directly from node ID and capability
    pub fn new(
        node_id: &str,
        capability: String,
        endpoint_hint: Option<String>,
        options: ClientOptions,
    ) -> Result<Self, ClientError> {
        let node_id: PublicKey = node_id
            .parse()
            .map_err(|e| ClientError::InvalidNodeId(format!("{}: {}", node_id, e)))?;

        Ok(Self {
            node_id,
            capability,
            endpoint_hint,
            options,
        })
    }

    /// Send a management request and receive response
    pub async fn request(
        &self,
        request: ManagementRequest,
    ) -> Result<ManagementResponse, ClientError> {
        let timeout = self.timeout_for_request(&request);

        tokio::time::timeout(timeout, self.send_request(request))
            .await
            .map_err(|_| ClientError::Timeout)?
    }

    /// Internal request sending logic
    async fn send_request(
        &self,
        request: ManagementRequest,
    ) -> Result<ManagementResponse, ClientError> {
        // Create Iroh endpoint
        let endpoint = Endpoint::builder()
            .alpns(vec![MANAGEMENT_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        // Build endpoint address from public key
        let addr = EndpointAddr::new(self.node_id);

        // Connect to node
        let conn = endpoint
            .connect(addr, MANAGEMENT_ALPN)
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        // Open bidirectional stream
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        // Build authenticated request
        let auth_request = AuthenticatedRequest {
            capability: self.capability.clone(),
            request,
        };

        // Serialize and send
        let request_bytes = serde_json::to_vec(&auth_request)?;
        write_frame(&mut send, &request_bytes).await?;
        send.finish()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        // Receive response
        let response_bytes = read_frame(&mut recv).await?;
        let response: ManagementResponse = serde_json::from_slice(&response_bytes)?;

        // Map error responses to ClientError
        if let ManagementResponse::Error { code, message } = &response {
            return Err(match code.as_str() {
                "UNAUTHORIZED" => ClientError::Unauthorized(message.clone()),
                "FORBIDDEN" => ClientError::Forbidden(message.clone()),
                "INVALID_REQUEST" => ClientError::InvalidRequest(message.clone()),
                "INVALID_STATE" => ClientError::InvalidState(message.clone()),
                _ => ClientError::ServerError(format!("{}: {}", code, message)),
            });
        }

        Ok(response)
    }

    /// Stream logs from the node (one-shot mode)
    pub async fn get_logs(&self, lines: u32) -> Result<Vec<String>, ClientError> {
        let response = self
            .request(ManagementRequest::StreamLogs {
                follow: false,
                lines: Some(lines),
            })
            .await?;

        match response {
            ManagementResponse::LogLines(lines) => Ok(lines),
            _ => Err(ClientError::ServerError(
                "Unexpected response type".to_string(),
            )),
        }
    }

    /// Stream logs with follow mode using a callback
    pub async fn stream_logs_with_callback<F>(
        &self,
        lines: u32,
        mut callback: F,
    ) -> Result<(), ClientError>
    where
        F: FnMut(String) -> bool,
    {
        // Create Iroh endpoint
        let endpoint = Endpoint::builder()
            .alpns(vec![MANAGEMENT_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        let addr = EndpointAddr::new(self.node_id);

        let conn = endpoint
            .connect(addr, MANAGEMENT_ALPN)
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        // Send StreamLogs request with follow=true
        let auth_request = AuthenticatedRequest {
            capability: self.capability.clone(),
            request: ManagementRequest::StreamLogs {
                follow: true,
                lines: Some(lines),
            },
        };

        let request_bytes = serde_json::to_vec(&auth_request)?;
        write_frame(&mut send, &request_bytes).await?;

        // Read log responses until callback returns false or connection closes
        loop {
            match read_frame(&mut recv).await {
                Ok(bytes) => match serde_json::from_slice::<ManagementResponse>(&bytes) {
                    Ok(ManagementResponse::LogLines(log_lines)) => {
                        for line in log_lines {
                            if !callback(line) {
                                return Ok(());
                            }
                        }
                    }
                    Ok(_) => break,
                    Err(e) => return Err(ClientError::Json(e)),
                },
                Err(_) => break,
            }
        }

        Ok(())
    }

    /// Determine timeout based on request type
    fn timeout_for_request(&self, request: &ManagementRequest) -> Duration {
        match request {
            // Long operations
            ManagementRequest::Register { .. }
            | ManagementRequest::Unregister
            | ManagementRequest::Join
            | ManagementRequest::Drain
            | ManagementRequest::Undrain
            | ManagementRequest::Upgrade { .. }
            | ManagementRequest::ApplyUpgrade
            | ManagementRequest::Reboot
            | ManagementRequest::ApplyConfig { .. } => self.options.long_timeout,

            // Short operations
            ManagementRequest::GetStatus
            | ManagementRequest::GetConfig
            | ManagementRequest::GetMetrics
            | ManagementRequest::StreamLogs { .. }
            | ManagementRequest::GenerateCapability { .. }
            | ManagementRequest::RevokeCapability { .. }
            | ManagementRequest::ListCapabilities => self.options.request_timeout,
        }
    }
}

/// Write a length-prefixed frame to the stream
async fn write_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> Result<(), ClientError> {
    let len = data.len() as u32;
    writer
        .write_all(&len.to_be_bytes())
        .await
        .map_err(ClientError::Io)?;
    writer.write_all(data).await.map_err(ClientError::Io)?;
    writer.flush().await.map_err(ClientError::Io)?;
    Ok(())
}

/// Read a length-prefixed frame from the stream
async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Vec<u8>, ClientError> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .map_err(ClientError::Io)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 10 * 1024 * 1024 {
        return Err(ClientError::WireError(format!(
            "Frame too large: {} bytes",
            len
        )));
    }

    let mut data = vec![0u8; len];
    reader
        .read_exact(&mut data)
        .await
        .map_err(ClientError::Io)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_options_default() {
        let opts = ClientOptions::default();
        assert_eq!(opts.request_timeout, Duration::from_secs(30));
        assert_eq!(opts.long_timeout, Duration::from_secs(300));
        assert_eq!(opts.retry_attempts, 3);
    }

    #[test]
    fn test_authenticated_request_serialization() {
        let req = AuthenticatedRequest {
            capability: "test-cap".to_string(),
            request: ManagementRequest::GetStatus,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("test-cap"));
        assert!(json.contains("get_status"));
    }
}
