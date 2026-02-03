//! Mock output processor for testing.
//!
//! Provides configurable behavior to simulate various output processing scenarios.

use super::OutputProcessor;
use crate::crypto::ChannelKeys;
use crate::executor::types::{ExecutionError, ExecutionRequest, ExecutionResult};
use async_trait::async_trait;
use iroh_blobs::Hash;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Configurable behavior for mock output processing.
#[derive(Debug, Clone, Default)]
pub enum MockOutputBehavior {
    /// Normal operation - returns successfully with mock encrypted data
    #[default]
    Normal,

    /// Fail all operations with a specific error
    AlwaysFail(String),

    /// Fail after N successful operations
    FailAfter(usize),

    /// Return empty output (simulates job that produces no output)
    EmptyOutput,

    /// Return specific encrypted data (for deterministic testing)
    FixedOutput {
        result: Vec<u8>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        hash: [u8; 32],
    },
}

/// Mock implementation of OutputProcessor for testing.
///
/// Allows configuring various behaviors to test error handling and edge cases.
#[derive(Debug, Clone)]
pub struct MockOutputProcessor {
    behavior: MockOutputBehavior,
    operation_count: Arc<AtomicUsize>,
}

impl Default for MockOutputProcessor {
    fn default() -> Self {
        Self::new(MockOutputBehavior::Normal)
    }
}

impl MockOutputProcessor {
    /// Create a new mock with specified behavior.
    pub fn new(behavior: MockOutputBehavior) -> Self {
        Self {
            behavior,
            operation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a mock that works normally.
    pub fn working() -> Self {
        Self::new(MockOutputBehavior::Normal)
    }

    /// Create a mock that always fails.
    pub fn failing(error: impl Into<String>) -> Self {
        Self::new(MockOutputBehavior::AlwaysFail(error.into()))
    }

    /// Create a mock that returns empty output.
    pub fn empty() -> Self {
        Self::new(MockOutputBehavior::EmptyOutput)
    }

    /// Create a mock with fixed output for deterministic testing.
    pub fn with_fixed_output(
        result: Vec<u8>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        hash: [u8; 32],
    ) -> Self {
        Self::new(MockOutputBehavior::FixedOutput {
            result,
            stdout,
            stderr,
            hash,
        })
    }

    /// Get the number of operations performed.
    pub fn operation_count(&self) -> usize {
        self.operation_count.load(Ordering::SeqCst)
    }

    fn check_should_fail(&self) -> Result<(), ExecutionError> {
        let count = self.operation_count.fetch_add(1, Ordering::SeqCst);

        match &self.behavior {
            MockOutputBehavior::Normal => Ok(()),
            MockOutputBehavior::EmptyOutput => Ok(()),
            MockOutputBehavior::FixedOutput { .. } => Ok(()),
            MockOutputBehavior::AlwaysFail(msg) => Err(ExecutionError::output(msg.clone())),
            MockOutputBehavior::FailAfter(n) if count >= *n => Err(ExecutionError::output(
                format!("Failed after {} operations", n),
            )),
            MockOutputBehavior::FailAfter(_) => Ok(()),
        }
    }
}

#[async_trait]
impl OutputProcessor for MockOutputProcessor {
    async fn process(
        &self,
        _drive_path: &Path,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
        duration: Duration,
        request: &ExecutionRequest,
        _channel_keys: &ChannelKeys,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.check_should_fail()?;

        // Generate mock encrypted data based on behavior
        let (encrypted_result, encrypted_stdout, encrypted_stderr, result_hash) =
            match &self.behavior {
                MockOutputBehavior::EmptyOutput => {
                    let hash = Hash::new(b"empty");
                    (vec![], vec![], vec![], hash)
                }
                MockOutputBehavior::FixedOutput {
                    result,
                    stdout: fixed_stdout,
                    stderr: fixed_stderr,
                    hash,
                } => (
                    result.clone(),
                    fixed_stdout.clone(),
                    fixed_stderr.clone(),
                    Hash::from_bytes(*hash),
                ),
                _ => {
                    // Normal behavior - generate mock encrypted data
                    let mock_result = format!("encrypted_result_{}", request.job_id).into_bytes();
                    let mock_stdout = if stdout.is_empty() {
                        vec![]
                    } else {
                        format!("encrypted_stdout_{}", request.job_id).into_bytes()
                    };
                    let mock_stderr = if stderr.is_empty() {
                        vec![]
                    } else {
                        format!("encrypted_stderr_{}", request.job_id).into_bytes()
                    };
                    let hash = Hash::new(&mock_result);
                    (mock_result, mock_stdout, mock_stderr, hash)
                }
            };

        Ok(ExecutionResult::new(
            exit_code,
            duration,
            encrypted_result,
            encrypted_stdout,
            encrypted_stderr,
            result_hash,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
    use crate::p2p::protocol::types::JobAssets;
    use std::collections::HashMap;

    fn make_test_request(job_id: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            job_id,
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets {
                code_hash: Hash::from_bytes([1u8; 32]),
                code_url: None,
                input_hash: Hash::from_bytes([2u8; 32]),
                input_url: None,
            },
            [0u8; 32],
            [0u8; 32],
            [0u8; 32],
            ResultDeliveryMode::Sync,
        )
    }

    fn mock_channel_keys() -> ChannelKeys {
        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];

        let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
        let worker_public = worker_signing.verifying_key().to_bytes();

        ChannelKeys::derive(&user_secret, &worker_public, &[3u8; 32]).unwrap()
    }

    #[tokio::test]
    async fn test_mock_normal_behavior() {
        let mock = MockOutputProcessor::working();
        let request = make_test_request("job-1");
        let channel_keys = mock_channel_keys();

        let result = mock
            .process(
                Path::new("/tmp/drive"),
                b"stdout".to_vec(),
                b"stderr".to_vec(),
                0,
                Duration::from_millis(100),
                &request,
                &channel_keys,
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.succeeded());
        assert!(!result.encrypted_result.is_empty());
        assert_eq!(mock.operation_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_always_fail() {
        let mock = MockOutputProcessor::failing("test error");
        let request = make_test_request("job-2");
        let channel_keys = mock_channel_keys();

        let result = mock
            .process(
                Path::new("/tmp/drive"),
                vec![],
                vec![],
                0,
                Duration::from_millis(100),
                &request,
                &channel_keys,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }

    #[tokio::test]
    async fn test_mock_fail_after() {
        let mock = MockOutputProcessor::new(MockOutputBehavior::FailAfter(2));
        let request = make_test_request("job-3");
        let channel_keys = mock_channel_keys();

        // First two operations succeed
        assert!(mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &channel_keys,
            )
            .await
            .is_ok());

        assert!(mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &channel_keys,
            )
            .await
            .is_ok());

        // Third operation fails
        assert!(mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &channel_keys,
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_mock_empty_output() {
        let mock = MockOutputProcessor::empty();
        let request = make_test_request("job-4");
        let channel_keys = mock_channel_keys();

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &channel_keys,
            )
            .await
            .unwrap();

        assert!(result.encrypted_result.is_empty());
        assert!(result.encrypted_stdout.is_empty());
        assert!(result.encrypted_stderr.is_empty());
    }

    #[tokio::test]
    async fn test_mock_fixed_output() {
        let fixed_result = b"fixed_result".to_vec();
        let fixed_stdout = b"fixed_stdout".to_vec();
        let fixed_stderr = b"fixed_stderr".to_vec();
        let fixed_hash = [42u8; 32];

        let mock = MockOutputProcessor::with_fixed_output(
            fixed_result.clone(),
            fixed_stdout.clone(),
            fixed_stderr.clone(),
            fixed_hash,
        );
        let request = make_test_request("job-5");
        let channel_keys = mock_channel_keys();

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &channel_keys,
            )
            .await
            .unwrap();

        assert_eq!(result.encrypted_result, fixed_result);
        assert_eq!(result.encrypted_stdout, fixed_stdout);
        assert_eq!(result.encrypted_stderr, fixed_stderr);
        assert_eq!(result.result_hash, Hash::from_bytes(fixed_hash));
    }

    #[tokio::test]
    async fn test_mock_preserves_exit_code() {
        let mock = MockOutputProcessor::working();
        let request = make_test_request("job-6");
        let channel_keys = mock_channel_keys();

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                b"error message".to_vec(),
                127,
                Duration::from_millis(50),
                &request,
                &channel_keys,
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 127);
        assert!(!result.succeeded());
    }

    #[tokio::test]
    async fn test_mock_preserves_duration() {
        let mock = MockOutputProcessor::working();
        let request = make_test_request("job-7");
        let channel_keys = mock_channel_keys();
        let duration = Duration::from_secs(5);

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                duration,
                &request,
                &channel_keys,
            )
            .await
            .unwrap();

        assert_eq!(result.duration, duration);
        assert_eq!(result.duration_ms(), 5000);
    }
}
