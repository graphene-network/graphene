//! Mock implementation of result delivery for testing.
//!
//! Provides configurable behavior for testing sync/async delivery,
//! fallback logic, and error conditions.

use async_trait::async_trait;
use iroh::EndpointAddr;
use iroh_blobs::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::{DeliveryError, DeliveryOutcome, EncryptedResult, ResultDelivery};
use crate::p2p::messages::ResultDeliveryMode;

/// Configurable behavior for mock delivery.
#[derive(Debug, Clone, Default)]
pub enum MockDeliveryBehavior {
    /// Always succeed with the specified mode.
    #[default]
    Success,
    /// Fail sync delivery (user offline), succeed async.
    SyncFailAsyncSuccess,
    /// Fail all delivery attempts.
    AlwaysFail,
    /// Timeout on delivery.
    Timeout,
}

/// Mock result delivery for testing.
pub struct MockResultDelivery {
    behavior: MockDeliveryBehavior,
    sync_attempts: AtomicUsize,
    async_attempts: AtomicUsize,
    delivered_results: Arc<Mutex<Vec<(String, DeliveryOutcome)>>>,
}

impl MockResultDelivery {
    /// Creates a new mock with default (success) behavior.
    pub fn new() -> Self {
        Self {
            behavior: MockDeliveryBehavior::Success,
            sync_attempts: AtomicUsize::new(0),
            async_attempts: AtomicUsize::new(0),
            delivered_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Creates a mock with specific behavior.
    pub fn with_behavior(behavior: MockDeliveryBehavior) -> Self {
        Self {
            behavior,
            sync_attempts: AtomicUsize::new(0),
            async_attempts: AtomicUsize::new(0),
            delivered_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns the number of sync delivery attempts.
    pub fn sync_attempts(&self) -> usize {
        self.sync_attempts.load(Ordering::SeqCst)
    }

    /// Returns the number of async delivery attempts.
    pub fn async_attempts(&self) -> usize {
        self.async_attempts.load(Ordering::SeqCst)
    }

    /// Returns all delivered results.
    pub async fn delivered_results(&self) -> Vec<(String, DeliveryOutcome)> {
        self.delivered_results.lock().await.clone()
    }
}

impl Default for MockResultDelivery {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResultDelivery for MockResultDelivery {
    async fn deliver_sync(
        &self,
        job_id: &str,
        _result: &EncryptedResult,
        _user_addr: &EndpointAddr,
    ) -> Result<(), DeliveryError> {
        self.sync_attempts.fetch_add(1, Ordering::SeqCst);

        match self.behavior {
            MockDeliveryBehavior::Success => {
                let mut results = self.delivered_results.lock().await;
                results.push((job_id.to_string(), DeliveryOutcome::SyncDelivered));
                Ok(())
            }
            MockDeliveryBehavior::SyncFailAsyncSuccess => Err(DeliveryError::UserOffline),
            MockDeliveryBehavior::AlwaysFail => {
                Err(DeliveryError::ConnectionError("mock failure".to_string()))
            }
            MockDeliveryBehavior::Timeout => Err(DeliveryError::Timeout),
        }
    }

    async fn deliver_async(
        &self,
        job_id: &str,
        _result: &EncryptedResult,
    ) -> Result<(Hash, Hash, Hash), DeliveryError> {
        self.async_attempts.fetch_add(1, Ordering::SeqCst);

        match self.behavior {
            MockDeliveryBehavior::Success | MockDeliveryBehavior::SyncFailAsyncSuccess => {
                let hash = Hash::new(job_id.as_bytes());
                let mut results = self.delivered_results.lock().await;
                results.push((
                    job_id.to_string(),
                    DeliveryOutcome::AsyncUploaded {
                        result_hash: hash,
                        stdout_hash: hash,
                        stderr_hash: hash,
                    },
                ));
                Ok((hash, hash, hash))
            }
            MockDeliveryBehavior::AlwaysFail => {
                Err(DeliveryError::BlobUploadError("mock failure".to_string()))
            }
            MockDeliveryBehavior::Timeout => Err(DeliveryError::Timeout),
        }
    }

    async fn deliver(
        &self,
        job_id: &str,
        result: &EncryptedResult,
        mode: ResultDeliveryMode,
        user_addr: Option<&EndpointAddr>,
        fallback: bool,
    ) -> Result<DeliveryOutcome, DeliveryError> {
        match mode {
            ResultDeliveryMode::Sync => {
                let addr = user_addr.ok_or(DeliveryError::UserOffline)?;
                match self.deliver_sync(job_id, result, addr).await {
                    Ok(()) => Ok(DeliveryOutcome::SyncDelivered),
                    Err(e) if fallback => {
                        // Fallback to async on sync failure
                        let (result_hash, stdout_hash, stderr_hash) =
                            self.deliver_async(job_id, result).await?;
                        Ok(DeliveryOutcome::AsyncUploaded {
                            result_hash,
                            stdout_hash,
                            stderr_hash,
                        })
                    }
                    Err(e) => Err(e),
                }
            }
            ResultDeliveryMode::Async => {
                let (result_hash, stdout_hash, stderr_hash) =
                    self.deliver_async(job_id, result).await?;
                Ok(DeliveryOutcome::AsyncUploaded {
                    result_hash,
                    stdout_hash,
                    stderr_hash,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_result() -> EncryptedResult {
        EncryptedResult {
            result: vec![1, 2, 3],
            stdout: vec![4, 5, 6],
            stderr: vec![7, 8, 9],
            exit_code: 0,
            execution_ms: 100,
        }
    }

    fn make_test_addr() -> EndpointAddr {
        // Create a minimal EndpointAddr for testing
        use iroh::PublicKey;
        let key = PublicKey::from_bytes(&[0u8; 32]).unwrap();
        EndpointAddr::from_parts(key, [])
    }

    #[tokio::test]
    async fn test_mock_sync_success() {
        let mock = MockResultDelivery::new();
        let result = make_test_result();
        let addr = make_test_addr();

        let outcome = mock
            .deliver(
                "job-1",
                &result,
                ResultDeliveryMode::Sync,
                Some(&addr),
                false,
            )
            .await
            .unwrap();

        assert!(outcome.is_sync());
        assert_eq!(mock.sync_attempts(), 1);
        assert_eq!(mock.async_attempts(), 0);
    }

    #[tokio::test]
    async fn test_mock_async_success() {
        let mock = MockResultDelivery::new();
        let result = make_test_result();

        let outcome = mock
            .deliver("job-1", &result, ResultDeliveryMode::Async, None, false)
            .await
            .unwrap();

        assert!(outcome.is_async());
        assert_eq!(mock.sync_attempts(), 0);
        assert_eq!(mock.async_attempts(), 1);
    }

    #[tokio::test]
    async fn test_mock_sync_fail_fallback_to_async() {
        let mock = MockResultDelivery::with_behavior(MockDeliveryBehavior::SyncFailAsyncSuccess);
        let result = make_test_result();
        let addr = make_test_addr();

        let outcome = mock
            .deliver(
                "job-1",
                &result,
                ResultDeliveryMode::Sync,
                Some(&addr),
                true,
            )
            .await
            .unwrap();

        assert!(outcome.is_async());
        assert_eq!(mock.sync_attempts(), 1);
        assert_eq!(mock.async_attempts(), 1);
    }

    #[tokio::test]
    async fn test_mock_sync_fail_no_fallback() {
        let mock = MockResultDelivery::with_behavior(MockDeliveryBehavior::SyncFailAsyncSuccess);
        let result = make_test_result();
        let addr = make_test_addr();

        let err = mock
            .deliver(
                "job-1",
                &result,
                ResultDeliveryMode::Sync,
                Some(&addr),
                false,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, DeliveryError::UserOffline));
        assert_eq!(mock.sync_attempts(), 1);
        assert_eq!(mock.async_attempts(), 0);
    }

    #[tokio::test]
    async fn test_mock_always_fail() {
        let mock = MockResultDelivery::with_behavior(MockDeliveryBehavior::AlwaysFail);
        let result = make_test_result();

        let err = mock
            .deliver("job-1", &result, ResultDeliveryMode::Async, None, false)
            .await
            .unwrap_err();

        assert!(matches!(err, DeliveryError::BlobUploadError(_)));
    }
}
