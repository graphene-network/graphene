//! HTTP-based management client for Graphene worker nodes.

#![allow(dead_code)]

use crate::config::NodeEntry;
use graphene_node::http::management::{ManagementRequest, ManagementResponse};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Client errors
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
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
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
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

/// Authenticated request wrapper for HTTP transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedRequest {
    pub capability: String,
    pub request: ManagementRequest,
}

/// Management client for connecting to Graphene worker nodes via HTTP.
pub struct ManagementClient {
    base_url: String,
    capability: String,
    client: reqwest::Client,
    options: ClientOptions,
}

impl ManagementClient {
    /// Create a client from a node entry configuration.
    pub fn from_config(entry: &NodeEntry, options: ClientOptions) -> Result<Self, ClientError> {
        let base_url = entry.url.clone();

        // Basic URL validation
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(ClientError::InvalidUrl(format!(
                "URL must start with http:// or https://: {}",
                base_url
            )));
        }

        let client = reqwest::Client::builder()
            .timeout(options.request_timeout)
            .build()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            capability: entry.capability.clone(),
            client,
            options,
        })
    }

    /// Create a client directly from a URL and capability.
    pub fn new(
        base_url: &str,
        capability: String,
        options: ClientOptions,
    ) -> Result<Self, ClientError> {
        let client = reqwest::Client::builder()
            .timeout(options.request_timeout)
            .build()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            capability,
            client,
            options,
        })
    }

    /// Send a management request and receive response.
    pub async fn request(
        &self,
        request: ManagementRequest,
    ) -> Result<ManagementResponse, ClientError> {
        let timeout = self.timeout_for_request(&request);
        let (method, path, body) = self.map_request(&request);

        let url = format!("{}{}", self.base_url, path);

        let mut req_builder = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            _ => self.client.get(&url),
        };

        req_builder = req_builder
            .header("Authorization", format!("Bearer {}", self.capability))
            .timeout(timeout);

        if let Some(body) = body {
            req_builder = req_builder
                .header("Content-Type", "application/json")
                .body(body);
        }

        let resp = req_builder
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ClientError::Timeout
                } else {
                    ClientError::ConnectionFailed(e.to_string())
                }
            })?;

        let status = resp.status();
        let body_bytes = resp.bytes().await.map_err(ClientError::Http)?;

        // Try to parse as ManagementResponse
        let response: ManagementResponse = serde_json::from_slice(&body_bytes)
            .map_err(|e| ClientError::ServerError(format!(
                "Failed to parse response (HTTP {}): {}",
                status, e
            )))?;

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

    /// Get logs from the node (one-shot mode).
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

    /// Stream logs with follow mode using a callback.
    ///
    /// Note: HTTP-based log streaming uses polling. For real-time streaming,
    /// consider WebSocket or SSE in a future version.
    pub async fn stream_logs_with_callback<F>(
        &self,
        lines: u32,
        mut callback: F,
    ) -> Result<(), ClientError>
    where
        F: FnMut(String) -> bool,
    {
        // In HTTP mode, we poll for logs instead of streaming
        let response = self
            .request(ManagementRequest::StreamLogs {
                follow: true,
                lines: Some(lines),
            })
            .await?;

        if let ManagementResponse::LogLines(log_lines) = response {
            for line in log_lines {
                if !callback(line) {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    /// Map a ManagementRequest to HTTP method, path, and optional JSON body.
    fn map_request(&self, request: &ManagementRequest) -> (&'static str, String, Option<String>) {
        match request {
            ManagementRequest::GetStatus => ("GET", "/v1/management/status".to_string(), None),
            ManagementRequest::GetConfig => ("GET", "/v1/management/config".to_string(), None),
            ManagementRequest::GetMetrics => ("GET", "/v1/management/metrics".to_string(), None),
            ManagementRequest::ApplyConfig { config } => (
                "POST",
                "/v1/management/config".to_string(),
                Some(serde_json::to_string(config).unwrap_or_default()),
            ),
            ManagementRequest::Register { .. }  => (
                "POST",
                "/v1/management/lifecycle/register".to_string(),
                Some(serde_json::to_string(request).unwrap_or_default()),
            ),
            ManagementRequest::Unregister => (
                "POST",
                "/v1/management/lifecycle/register".to_string(),
                None,
            ),
            ManagementRequest::Join => (
                "POST",
                "/v1/management/lifecycle/join".to_string(),
                None,
            ),
            ManagementRequest::Drain => (
                "POST",
                "/v1/management/lifecycle/drain".to_string(),
                None,
            ),
            ManagementRequest::Undrain => (
                "POST",
                "/v1/management/lifecycle/undrain".to_string(),
                None,
            ),
            ManagementRequest::Reboot => (
                "POST",
                "/v1/management/lifecycle/reboot".to_string(),
                None,
            ),
            ManagementRequest::StreamLogs { .. } => {
                // TODO(#200): Implement log streaming endpoint
                ("GET", "/v1/management/logs".to_string(), None)
            }
            ManagementRequest::Upgrade { .. } => (
                "POST",
                "/v1/management/upgrade".to_string(),
                Some(serde_json::to_string(request).unwrap_or_default()),
            ),
            ManagementRequest::ApplyUpgrade => (
                "POST",
                "/v1/management/upgrade/apply".to_string(),
                None,
            ),
            ManagementRequest::GenerateCapability { .. } => (
                "POST",
                "/v1/management/capabilities".to_string(),
                Some(serde_json::to_string(request).unwrap_or_default()),
            ),
            ManagementRequest::RevokeCapability { token_prefix } => (
                "POST",
                format!("/v1/management/capabilities/{}/revoke", token_prefix),
                None,
            ),
            ManagementRequest::ListCapabilities => (
                "GET",
                "/v1/management/capabilities".to_string(),
                None,
            ),
        }
    }

    /// Determine timeout based on request type.
    fn timeout_for_request(&self, request: &ManagementRequest) -> Duration {
        match request {
            ManagementRequest::Register { .. }
            | ManagementRequest::Unregister
            | ManagementRequest::Join
            | ManagementRequest::Drain
            | ManagementRequest::Undrain
            | ManagementRequest::Upgrade { .. }
            | ManagementRequest::ApplyUpgrade
            | ManagementRequest::Reboot
            | ManagementRequest::ApplyConfig { .. } => self.options.long_timeout,
            _ => self.options.request_timeout,
        }
    }
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

    #[test]
    fn test_client_invalid_url() {
        let entry = NodeEntry {
            url: "not-a-url".to_string(),
            capability: "cap".to_string(),
            description: None,
        };
        let result = ManagementClient::from_config(&entry, ClientOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_client_valid_url() {
        let entry = NodeEntry {
            url: "http://localhost:9000".to_string(),
            capability: "cap".to_string(),
            description: None,
        };
        let result = ManagementClient::from_config(&entry, ClientOptions::default());
        assert!(result.is_ok());
    }
}
