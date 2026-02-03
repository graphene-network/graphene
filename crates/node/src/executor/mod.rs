//! Job executor module for running jobs in isolated unikernel environments.
//!
//! This module provides the core abstraction for job execution in Graphene.
//! The [`JobExecutor`] trait defines the interface for running jobs, with
//! implementations for production (Firecracker) and testing (mock).
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
//! │ 1. Fetch assets │
//! │ 2. Decrypt      │
//! │ 3. Build/cache  │
//! │ 4. Create drive │
//! │ 5. Run VMM      │
//! │ 6. Capture out  │
//! │ 7. Encrypt      │
//! └─────────────────┘
//!        │
//!        ▼
//! ExecutionResult
//! ```
//!
//! # Example
//!
//! ```ignore
//! use monad_node::executor::{JobExecutor, ExecutionRequest, ExecutionResult};
//!
//! async fn run_job(executor: &impl JobExecutor, request: ExecutionRequest) {
//!     match executor.execute(request).await {
//!         Ok(result) => {
//!             println!("Job completed with exit code: {}", result.exit_code);
//!             println!("Duration: {:?}", result.duration);
//!         }
//!         Err(e) => {
//!             eprintln!("Execution failed: {}", e);
//!         }
//!     }
//! }
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
///
/// Implementations are responsible for the full execution pipeline:
/// 1. Fetching encrypted code and input assets
/// 2. Decrypting assets using the ephemeral key exchange
/// 3. Looking up or building the unikernel
/// 4. Creating the ext4 drive with code and input
/// 5. Running the VMM with resource limits
/// 6. Capturing stdout/stderr and return value
/// 7. Encrypting outputs for the user
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow concurrent job execution.
/// Each job runs in complete isolation via Firecracker MicroVMs.
///
/// # Cancellation
///
/// Implementations should respect cancellation requests and return
/// [`ExecutionError::Cancelled`] when the job is stopped early.
#[async_trait]
pub trait JobExecutor: Send + Sync {
    /// Execute a job and return the result.
    ///
    /// This is the main entry point for job execution. The implementation
    /// handles all phases from asset fetching through result encryption.
    ///
    /// # Arguments
    ///
    /// * `request` - The execution request containing job configuration and assets
    ///
    /// # Returns
    ///
    /// * `Ok(ExecutionResult)` - Job completed (check exit_code for success/failure)
    /// * `Err(ExecutionError)` - Job could not complete due to infrastructure error
    ///
    /// # Errors
    ///
    /// Returns errors for infrastructure failures. User code errors (non-zero exit)
    /// are returned as successful results with the appropriate exit code.
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, ExecutionError>;

    /// Cancel a running job.
    ///
    /// Attempts to stop a job that is currently executing. This is best-effort;
    /// the job may complete before the cancellation takes effect.
    ///
    /// # Arguments
    ///
    /// * `job_id` - The ID of the job to cancel
    ///
    /// # Returns
    ///
    /// * `true` - Job was found and cancellation was initiated
    /// * `false` - Job was not found (already completed or never started)
    async fn cancel(&self, job_id: &str) -> bool;

    /// Check if a job is currently running.
    ///
    /// # Arguments
    ///
    /// * `job_id` - The ID of the job to check
    ///
    /// # Returns
    ///
    /// * `true` - Job is currently executing
    /// * `false` - Job is not running (completed, cancelled, or never started)
    async fn is_running(&self, job_id: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
    use crate::p2p::protocol::types::JobAssets;
    use iroh_blobs::Hash;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Simple mock executor for testing the trait interface.
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
            // Track that job is running
            {
                let mut jobs = self.running_jobs.lock().unwrap();
                jobs.insert(request.job_id.clone());
            }

            // Simulate some work
            tokio::time::sleep(Duration::from_millis(10)).await;

            // Clean up
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
                Hash::new(b"result"),
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
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
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

        // Job not running yet
        assert!(!executor.is_running("job-3").await);
        assert!(!executor.cancel("job-3").await);
    }

    #[tokio::test]
    async fn test_trait_is_object_safe() {
        // Verify the trait can be used as a trait object
        let executor: Box<dyn JobExecutor> = Box::new(MockExecutor::new());
        let request = make_test_request("job-4");
        let result = executor.execute(request).await;
        assert!(result.is_ok());
    }
}
