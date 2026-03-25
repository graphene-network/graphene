//! Job executor module for running jobs in isolated unikernel environments.
//!
//! # Architecture
//!
//! ```text
//! ExecutionRequest
//!        │
//!        ▼
//! ┌─────────────────┐
//! │  JobExecutor    │
//! │                 │
//! │ 1. Receive code │
//! │ 2. Build/cache  │
//! │ 3. Create drive │
//! │ 4. Run VMM      │
//! │ 5. Capture out  │
//! └─────────────────┘
//!        │
//!        ▼
//! ExecutionResult
//! ```

mod default;
pub mod drive;
pub mod output;
pub mod runner;
pub mod types;

pub use default::mock::{MockExecutorBehavior, MockJobExecutor};
pub use default::{DefaultJobExecutor, ExecutorConfig};
pub use drive::{build_env_json, paths, DriveConfig, ExecutionDriveBuilder};
pub use output::{
    DefaultOutputProcessor, MockOutputBehavior, MockOutputProcessor, OutputProcessor,
};
pub use runner::{
    FirecrackerRunner, FirecrackerRunnerConfig, MockRunner, MockRunnerBehavior, MockRunnerBuilder,
    MockRunnerCall, RunnerError, VmmOutput, VmmRunner,
};
pub use types::{reserved_env, ExecutionError, ExecutionRequest, ExecutionResult};

use async_trait::async_trait;

/// Trait for executing jobs in isolated unikernel environments.
#[async_trait]
pub trait JobExecutor: Send + Sync {
    /// Execute a job and return the result.
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, ExecutionError>;

    /// Cancel a running job.
    async fn cancel(&self, job_id: &str) -> bool;

    /// Check if a job is currently running.
    async fn is_running(&self, job_id: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JobAssets, JobManifest};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct MockExecutor {
        should_fail: AtomicBool,
        running_jobs: std::sync::Mutex<std::collections::HashSet<String>>,
    }

    impl MockExecutor {
        fn new() -> Self {
            Self {
                should_fail: AtomicBool::new(false),
                running_jobs: std::sync::Mutex::new(std::collections::HashSet::new()),
            }
        }

        fn set_should_fail(&self, fail: bool) {
            self.should_fail.store(fail, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl JobExecutor for MockExecutor {
        async fn execute(
            &self,
            request: ExecutionRequest,
        ) -> Result<ExecutionResult, ExecutionError> {
            {
                let mut jobs = self.running_jobs.lock().unwrap();
                jobs.insert(request.job_id.clone());
            }

            tokio::time::sleep(Duration::from_millis(10)).await;

            {
                let mut jobs = self.running_jobs.lock().unwrap();
                jobs.remove(&request.job_id);
            }

            if self.should_fail.load(Ordering::SeqCst) {
                return Err(ExecutionError::vmm("mock failure"));
            }

            Ok(ExecutionResult::new(
                0,
                Duration::from_millis(100),
                b"result".to_vec(),
                b"stdout".to_vec(),
                vec![],
            ))
        }

        async fn cancel(&self, job_id: &str) -> bool {
            let mut jobs = self.running_jobs.lock().unwrap();
            jobs.remove(job_id)
        }

        async fn is_running(&self, job_id: &str) -> bool {
            let jobs = self.running_jobs.lock().unwrap();
            jobs.contains(job_id)
        }
    }

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
    async fn test_mock_executor_success() {
        let executor = MockExecutor::new();
        let request = make_test_request("job-1");

        let result = executor.execute(request).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.succeeded());
    }

    #[tokio::test]
    async fn test_mock_executor_failure() {
        let executor = MockExecutor::new();
        executor.set_should_fail(true);
        let request = make_test_request("job-2");

        let result = executor.execute(request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExecutionError::VmmError(_)));
    }

    #[tokio::test]
    async fn test_mock_executor_cancel() {
        let executor = Arc::new(MockExecutor::new());
        assert!(!executor.is_running("job-3").await);
        assert!(!executor.cancel("job-3").await);
    }

    #[tokio::test]
    async fn test_trait_is_object_safe() {
        let executor: Box<dyn JobExecutor> = Box::new(MockExecutor::new());
        let request = make_test_request("job-4");
        let result = executor.execute(request).await;
        assert!(result.is_ok());
    }
}
