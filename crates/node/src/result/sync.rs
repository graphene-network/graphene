//! Synchronous result delivery via direct QUIC streaming.
//!
//! This module implements low-latency result delivery by streaming encrypted
//! results directly to the user over a QUIC connection.

use async_trait::async_trait;
use iroh::EndpointAddr;
use iroh_blobs::Hash;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, warn};

use super::{DeliveryError, DeliveryOutcome, EncryptedResult, ResultDelivery};
use crate::p2p::messages::ResultDeliveryMode;
use crate::p2p::P2PNetwork;

/// ALPN protocol identifier for result delivery streams.
pub const RESULT_DELIVERY_ALPN: &[u8] = b"graphene-result/1";

/// Default timeout for sync delivery attempts.
pub const DEFAULT_SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// Synchronous result delivery using QUIC streams.
///
/// Streams encrypted results directly to users for lowest latency delivery.
pub struct SyncDelivery<N: P2PNetwork> {
    network: Arc<N>,
    timeout: Duration,
}

impl<N: P2PNetwork> SyncDelivery<N> {
    /// Creates a new sync delivery handler.
    pub fn new(network: Arc<N>) -> Self {
        Self {
            network,
            timeout: DEFAULT_SYNC_TIMEOUT,
        }
    }

    /// Creates a sync delivery handler with a custom timeout.
    pub fn with_timeout(network: Arc<N>, timeout: Duration) -> Self {
        Self { network, timeout }
    }
}

#[async_trait]
impl<N: P2PNetwork + 'static> ResultDelivery for SyncDelivery<N> {
    async fn deliver_sync(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        user_addr: &EndpointAddr,
    ) -> Result<(), DeliveryError> {
        debug!(job_id, ?user_addr, "Attempting sync delivery");

        // Establish QUIC connection to user
        let conn = timeout(
            self.timeout,
            self.network
                .connect(user_addr.clone(), RESULT_DELIVERY_ALPN),
        )
        .await
        .map_err(|_| DeliveryError::Timeout)?
        .map_err(|e| {
            warn!(job_id, error = %e, "Failed to connect for sync delivery");
            DeliveryError::ConnectionError(e.to_string())
        })?;

        // Open a unidirectional stream for result delivery
        let mut send_stream = conn
            .open_uni()
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Write job ID length and job ID
        let job_id_bytes = job_id.as_bytes();
        send_stream
            .write_all(&(job_id_bytes.len() as u32).to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;
        send_stream
            .write_all(job_id_bytes)
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Write exit code and execution time
        send_stream
            .write_all(&result.exit_code.to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;
        send_stream
            .write_all(&result.execution_ms.to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Write result data (length-prefixed)
        send_stream
            .write_all(&(result.result.len() as u64).to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;
        send_stream
            .write_all(&result.result)
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Write stdout data (length-prefixed)
        send_stream
            .write_all(&(result.stdout.len() as u64).to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;
        send_stream
            .write_all(&result.stdout)
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Write stderr data (length-prefixed)
        send_stream
            .write_all(&(result.stderr.len() as u64).to_le_bytes())
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;
        send_stream
            .write_all(&result.stderr)
            .await
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        // Finish the stream
        send_stream
            .finish()
            .map_err(|e| DeliveryError::StreamError(e.to_string()))?;

        debug!(job_id, "Sync delivery completed");
        Ok(())
    }

    async fn deliver_async(
        &self,
        _job_id: &str,
        _result: &EncryptedResult,
    ) -> Result<(Hash, Hash, Hash), DeliveryError> {
        // SyncDelivery doesn't support async mode - use AsyncDelivery instead
        Err(DeliveryError::StreamError(
            "SyncDelivery does not support async mode".to_string(),
        ))
    }

    async fn deliver(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        mode: ResultDeliveryMode,
        user_addr: Option<&EndpointAddr>,
        _fallback: bool,
    ) -> Result<DeliveryOutcome, DeliveryError> {
        match mode {
            ResultDeliveryMode::Sync => {
                let addr = user_addr.ok_or(DeliveryError::UserOffline)?;
                self.deliver_sync(job_id, result, addr).await?;
                Ok(DeliveryOutcome::SyncDelivered)
            }
            ResultDeliveryMode::Async => Err(DeliveryError::StreamError(
                "SyncDelivery does not support async mode".to_string(),
            )),
        }
    }
}
