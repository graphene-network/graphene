//! Mock output processor for testing.

use super::OutputProcessor;
use crate::executor::types::{ExecutionError, ExecutionResult};
use async_trait::async_trait;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Configurable behavior for mock output processing.
#[derive(Debug, Clone, Default)]
pub enum MockOutputBehavior {
    #[default]
    Normal,
    AlwaysFail(String),
    FailAfter(usize),
    EmptyOutput,
    FixedOutput {
        result: Vec<u8>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

/// Mock implementation of OutputProcessor for testing.
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
    pub fn new(behavior: MockOutputBehavior) -> Self {
        Self {
            behavior,
            operation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn working() -> Self {
        Self::new(MockOutputBehavior::Normal)
    }

    pub fn failing(error: impl Into<String>) -> Self {
        Self::new(MockOutputBehavior::AlwaysFail(error.into()))
    }

    pub fn empty() -> Self {
        Self::new(MockOutputBehavior::EmptyOutput)
    }

    pub fn with_fixed_output(result: Vec<u8>, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self::new(MockOutputBehavior::FixedOutput {
            result,
            stdout,
            stderr,
        })
    }

    pub fn operation_count(&self) -> usize {
        self.operation_count.load(Ordering::SeqCst)
    }

    fn check_should_fail(&self) -> Result<(), ExecutionError> {
        let count = self.operation_count.fetch_add(1, Ordering::SeqCst);

        match &self.behavior {
            MockOutputBehavior::Normal
            | MockOutputBehavior::EmptyOutput
            | MockOutputBehavior::FixedOutput { .. } => Ok(()),
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
    ) -> Result<ExecutionResult, ExecutionError> {
        self.check_should_fail()?;

        let (result, out, err) = match &self.behavior {
            MockOutputBehavior::EmptyOutput => (vec![], vec![], vec![]),
            MockOutputBehavior::FixedOutput {
                result,
                stdout: fixed_stdout,
                stderr: fixed_stderr,
            } => (result.clone(), fixed_stdout.clone(), fixed_stderr.clone()),
            _ => {
                let mock_result = b"mock_result".to_vec();
                let mock_stdout = if stdout.is_empty() {
                    vec![]
                } else {
                    stdout
                };
                let mock_stderr = if stderr.is_empty() {
                    vec![]
                } else {
                    stderr
                };
                (mock_result, mock_stdout, mock_stderr)
            }
        };

        Ok(ExecutionResult::new(exit_code, duration, result, out, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::types::ExecutionRequest;
    use crate::types::{JobAssets, JobManifest};
    use std::collections::HashMap;

    fn make_test_request(job_id: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            job_id,
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                runtime: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets::inline(b"print('hi')".to_vec(), None),
        )
    }

    #[tokio::test]
    async fn test_mock_normal_behavior() {
        let mock = MockOutputProcessor::working();

        let result = mock
            .process(
                Path::new("/tmp/drive"),
                b"stdout".to_vec(),
                b"stderr".to_vec(),
                0,
                Duration::from_millis(100),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.succeeded());
        assert!(!result.result.is_empty());
        assert_eq!(mock.operation_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_always_fail() {
        let mock = MockOutputProcessor::failing("test error");

        let result = mock
            .process(
                Path::new("/tmp/drive"),
                vec![],
                vec![],
                0,
                Duration::from_millis(100),
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }

    #[tokio::test]
    async fn test_mock_empty_output() {
        let mock = MockOutputProcessor::empty();

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
            )
            .await
            .unwrap();

        assert!(result.result.is_empty());
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_mock_preserves_exit_code() {
        let mock = MockOutputProcessor::working();

        let result = mock
            .process(
                Path::new("/tmp"),
                vec![],
                b"error message".to_vec(),
                127,
                Duration::from_millis(50),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 127);
        assert!(!result.succeeded());
    }

    #[tokio::test]
    async fn test_mock_preserves_duration() {
        let mock = MockOutputProcessor::working();
        let duration = Duration::from_secs(5);

        let result = mock
            .process(Path::new("/tmp"), vec![], vec![], 0, duration)
            .await
            .unwrap();

        assert_eq!(result.duration, duration);
        assert_eq!(result.duration_ms(), 5000);
    }
}
