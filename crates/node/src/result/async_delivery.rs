//! Asynchronous result delivery via Iroh blob storage.
//!
//! This module implements result delivery by uploading encrypted results
//! to the Iroh blob store for later retrieval by the user.

use async_trait::async_trait;
use iroh::EndpointAddr;
use iroh_blobs::Hash;
use std::sync::Arc;
use tracing::{debug, warn};

use super::{DeliveryError, DeliveryOutcome, EncryptedResult, ResultDelivery};
use crate::p2p::messages::ResultDeliveryMode;
use crate::p2p::P2PNetwork;

/// Asynchronous result delivery using Iroh blob storage.
///
/// Uploads encrypted results to the local Iroh blob store, making them
/// available for P2P retrieval with a 24-hour TTL.
pub struct AsyncDelivery<N: P2PNetwork> {
    network: Arc<N>,
}

impl<N: P2PNetwork> AsyncDelivery<N> {
    /// Creates a new async delivery handler.
    pub fn new(network: Arc<N>) -> Self {
        Self { network }
    }
}

#[async_trait]
impl<N: P2PNetwork + 'static> ResultDelivery for AsyncDelivery<N> {
    async fn deliver_sync(
        &self,
        _job_id: &str,
        _result: &EncryptedResult,
        _user_addr: &EndpointAddr,
    ) -> Result<(), DeliveryError> {
        // AsyncDelivery doesn't support sync mode - use SyncDelivery instead
        Err(DeliveryError::StreamError(
            "AsyncDelivery does not support sync mode".to_string(),
        ))
    }

    async fn deliver_async(
        &self,
        job_id: &str,
        result: &EncryptedResult,
    ) -> Result<(Hash, Hash, Hash), DeliveryError> {
        debug!(job_id, "Uploading result to Iroh blob store");

        // Upload encrypted result
        let result_hash = self
            .network
            .upload_blob(&result.result)
            .await
            .map_err(|e| {
                warn!(job_id, error = %e, "Failed to upload result blob");
                DeliveryError::BlobUploadError(e.to_string())
            })?;

        // Upload encrypted stdout
        let stdout_hash = self
            .network
            .upload_blob(&result.stdout)
            .await
            .map_err(|e| {
                warn!(job_id, error = %e, "Failed to upload stdout blob");
                DeliveryError::BlobUploadError(e.to_string())
            })?;

        // Upload encrypted stderr
        let stderr_hash = self
            .network
            .upload_blob(&result.stderr)
            .await
            .map_err(|e| {
                warn!(job_id, error = %e, "Failed to upload stderr blob");
                DeliveryError::BlobUploadError(e.to_string())
            })?;

        debug!(
            job_id,
            %result_hash,
            %stdout_hash,
            %stderr_hash,
            "Async delivery completed"
        );

        Ok((result_hash, stdout_hash, stderr_hash))
    }

    async fn deliver(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        mode: ResultDeliveryMode,
        _user_addr: Option<&EndpointAddr>,
        _fallback: bool,
    ) -> Result<DeliveryOutcome, DeliveryError> {
        match mode {
            ResultDeliveryMode::Async => {
                let (result_hash, stdout_hash, stderr_hash) =
                    self.deliver_async(job_id, result).await?;
                Ok(DeliveryOutcome::AsyncUploaded {
                    result_hash,
                    stdout_hash,
                    stderr_hash,
                })
            }
            ResultDeliveryMode::Sync => Err(DeliveryError::StreamError(
                "AsyncDelivery does not support sync mode".to_string(),
            )),
        }
    }
}
