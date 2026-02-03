//! Mock implementation of VmmRunner for testing.
//!
//! This module provides a configurable mock runner that can simulate
//! various execution scenarios without requiring actual Firecracker VMs.

use super::{RunnerError, VmmOutput, VmmRunner};
use crate::p2p::messages::JobManifest;
use async_trait::async_trait;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

/// Configurable behaviors for the mock runner.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub enum MockRunnerBehavior {
    /// Job completes successfully with exit code 0.
    Success {
        /// Output to return as stdout.
        stdout: Vec<u8>,
        /// Simulated execution duration.
        duration: Duration,
    },

    /// Job completes with a non-zero exit code.
    Failure {
        /// Exit code to return.
        exit_code: i32,
        /// Output to return as stderr.
        stderr: Vec<u8>,
        /// Simulated execution duration.
        duration: Duration,
    },

    /// Job times out (does not complete within timeout).
    Timeout {
        /// Partial output captured before timeout.
        partial_output: Vec<u8>,
    },

    /// VM crashes during execution.
    Crash {
        /// Error message for the crash.
        message: String,
    },

    /// VM fails to start (configuration error).
    ConfigurationError {
        /// Error message.
        message: String,
    },

    /// VM fails during boot.
    BootError {
        /// Error message.
        message: String,
    },

    /// Custom behavior defined by a closure.
    /// Takes (kernel_path, drive_path, manifest) and returns Result<VmmOutput, RunnerError>.
    Custom(Arc<dyn Fn(&Path, &Path, &JobManifest) -> Result<VmmOutput, RunnerError> + Send + Sync>),
}

impl std::fmt::Debug for MockRunnerBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success { stdout, duration } => f
                .debug_struct("Success")
                .field("stdout", &format!("{} bytes", stdout.len()))
                .field("duration", duration)
                .finish(),
            Self::Failure {
                exit_code,
                stderr,
                duration,
            } => f
                .debug_struct("Failure")
                .field("exit_code", exit_code)
                .field("stderr", &format!("{} bytes", stderr.len()))
                .field("duration", duration)
                .finish(),
            Self::Timeout { partial_output } => f
                .debug_struct("Timeout")
                .field("partial_output", &format!("{} bytes", partial_output.len()))
                .finish(),
            Self::Crash { message } => f.debug_struct("Crash").field("message", message).finish(),
            Self::ConfigurationError { message } => f
                .debug_struct("ConfigurationError")
                .field("message", message)
                .finish(),
            Self::BootError { message } => f
                .debug_struct("BootError")
                .field("message", message)
                .finish(),
            Self::Custom(_) => f.debug_struct("Custom").finish_non_exhaustive(),
        }
    }
}

impl Default for MockRunnerBehavior {
    fn default() -> Self {
        Self::Success {
            stdout: b"Mock execution completed\n".to_vec(),
            duration: Duration::from_millis(100),
        }
    }
}

/// Mock implementation of VmmRunner for testing.
///
/// # Example
///
/// ```ignore
/// use monad_node::executor::runner::mock::{MockRunner, MockRunnerBehavior};
///
/// // Create a mock that succeeds
/// let runner = MockRunner::new(MockRunnerBehavior::Success {
///     stdout: b"Hello, World!\n".to_vec(),
///     duration: Duration::from_millis(50),
/// });
///
/// // Create a mock that times out
/// let runner = MockRunner::new(MockRunnerBehavior::Timeout {
///     partial_output: b"Starting...\n".to_vec(),
/// });
/// ```
pub struct MockRunner {
    behavior: Mutex<MockRunnerBehavior>,
    call_count: AtomicU32,
    calls: Mutex<Vec<MockRunnerCall>>,
}

/// Record of a call made to the mock runner.
#[derive(Debug, Clone)]
pub struct MockRunnerCall {
    pub kernel_path: String,
    pub drive_path: String,
    pub vcpu: u8,
    pub memory_mb: u32,
    pub timeout_ms: u64,
}

impl MockRunner {
    /// Creates a new mock runner with the specified behavior.
    pub fn new(behavior: MockRunnerBehavior) -> Self {
        Self {
            behavior: Mutex::new(behavior),
            call_count: AtomicU32::new(0),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Creates a mock runner that always succeeds.
    pub fn success() -> Self {
        Self::new(MockRunnerBehavior::default())
    }

    /// Creates a mock runner that always fails with the given exit code.
    pub fn failure(exit_code: i32) -> Self {
        Self::new(MockRunnerBehavior::Failure {
            exit_code,
            stderr: format!("Job failed with exit code {}\n", exit_code).into_bytes(),
            duration: Duration::from_millis(100),
        })
    }

    /// Creates a mock runner that always times out.
    pub fn timeout() -> Self {
        Self::new(MockRunnerBehavior::Timeout {
            partial_output: b"Execution timed out\n".to_vec(),
        })
    }

    /// Creates a mock runner that crashes.
    pub fn crash(message: impl Into<String>) -> Self {
        Self::new(MockRunnerBehavior::Crash {
            message: message.into(),
        })
    }

    /// Sets a new behavior for subsequent calls.
    pub fn set_behavior(&self, behavior: MockRunnerBehavior) {
        let mut guard = self.behavior.lock().unwrap();
        *guard = behavior;
    }

    /// Returns the number of times `run` has been called.
    pub fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Returns a copy of all recorded calls.
    pub fn calls(&self) -> Vec<MockRunnerCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clears the call history.
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
        self.call_count.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl VmmRunner for MockRunner {
    async fn run(
        &self,
        kernel_path: &Path,
        drive_path: &Path,
        manifest: &JobManifest,
        _boot_args: &str,
    ) -> Result<VmmOutput, RunnerError> {
        // Record the call
        self.call_count.fetch_add(1, Ordering::SeqCst);
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push(MockRunnerCall {
                kernel_path: kernel_path.display().to_string(),
                drive_path: drive_path.display().to_string(),
                vcpu: manifest.vcpu,
                memory_mb: manifest.memory_mb,
                timeout_ms: manifest.timeout_ms,
            });
        }

        let behavior = self.behavior.lock().unwrap().clone();

        match behavior {
            MockRunnerBehavior::Success { stdout, duration } => {
                sleep(duration).await;
                Ok(VmmOutput::new(0, stdout, vec![], duration, false))
            }

            MockRunnerBehavior::Failure {
                exit_code,
                stderr,
                duration,
            } => {
                sleep(duration).await;
                Ok(VmmOutput::new(exit_code, vec![], stderr, duration, false))
            }

            MockRunnerBehavior::Timeout { partial_output } => {
                // Simulate actual timeout duration
                let timeout_duration = Duration::from_millis(manifest.timeout_ms);
                sleep(timeout_duration).await;
                Ok(VmmOutput::new(
                    -1,
                    partial_output,
                    vec![],
                    timeout_duration,
                    true,
                ))
            }

            MockRunnerBehavior::Crash { message } => Err(RunnerError::Crashed(message)),

            MockRunnerBehavior::ConfigurationError { message } => {
                Err(RunnerError::ConfigurationFailed(message))
            }

            MockRunnerBehavior::BootError { message } => Err(RunnerError::StartFailed(message)),

            MockRunnerBehavior::Custom(handler) => handler(kernel_path, drive_path, manifest),
        }
    }
}

/// Builder for creating mock runners with fluent API.
pub struct MockRunnerBuilder {
    behavior: MockRunnerBehavior,
}

impl MockRunnerBuilder {
    /// Creates a new builder with default success behavior.
    pub fn new() -> Self {
        Self {
            behavior: MockRunnerBehavior::default(),
        }
    }

    /// Sets the behavior to success with the given stdout.
    pub fn with_success(mut self, stdout: impl Into<Vec<u8>>) -> Self {
        self.behavior = MockRunnerBehavior::Success {
            stdout: stdout.into(),
            duration: Duration::from_millis(100),
        };
        self
    }

    /// Sets the behavior to success with custom duration.
    pub fn with_success_duration(mut self, stdout: impl Into<Vec<u8>>, duration: Duration) -> Self {
        self.behavior = MockRunnerBehavior::Success {
            stdout: stdout.into(),
            duration,
        };
        self
    }

    /// Sets the behavior to failure with the given exit code and stderr.
    pub fn with_failure(mut self, exit_code: i32, stderr: impl Into<Vec<u8>>) -> Self {
        self.behavior = MockRunnerBehavior::Failure {
            exit_code,
            stderr: stderr.into(),
            duration: Duration::from_millis(100),
        };
        self
    }

    /// Sets the behavior to timeout.
    pub fn with_timeout(mut self) -> Self {
        self.behavior = MockRunnerBehavior::Timeout {
            partial_output: vec![],
        };
        self
    }

    /// Sets the behavior to timeout with partial output.
    pub fn with_timeout_output(mut self, partial: impl Into<Vec<u8>>) -> Self {
        self.behavior = MockRunnerBehavior::Timeout {
            partial_output: partial.into(),
        };
        self
    }

    /// Sets the behavior to crash.
    pub fn with_crash(mut self, message: impl Into<String>) -> Self {
        self.behavior = MockRunnerBehavior::Crash {
            message: message.into(),
        };
        self
    }

    /// Builds the mock runner.
    pub fn build(self) -> MockRunner {
        MockRunner::new(self.behavior)
    }
}

impl Default for MockRunnerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 5000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    #[tokio::test]
    async fn test_mock_runner_success() {
        let runner = MockRunner::success();
        let manifest = make_test_manifest();

        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await
            .unwrap();

        assert!(result.succeeded());
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);
        assert_eq!(runner.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_runner_failure() {
        let runner = MockRunner::failure(42);
        let manifest = make_test_manifest();

        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await
            .unwrap();

        assert!(!result.succeeded());
        assert_eq!(result.exit_code, 42);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_mock_runner_crash() {
        let runner = MockRunner::crash("kernel panic");
        let manifest = make_test_manifest();

        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await;

        assert!(matches!(result, Err(RunnerError::Crashed(_))));
    }

    #[tokio::test]
    async fn test_mock_runner_call_recording() {
        let runner = MockRunner::success();
        let manifest = make_test_manifest();

        runner
            .run(
                Path::new("/path/to/kernel"),
                Path::new("/path/to/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await
            .unwrap();

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].kernel_path, "/path/to/kernel");
        assert_eq!(calls[0].drive_path, "/path/to/drive");
        assert_eq!(calls[0].vcpu, 2);
        assert_eq!(calls[0].memory_mb, 512);
    }

    #[tokio::test]
    async fn test_mock_runner_builder() {
        let runner = MockRunnerBuilder::new()
            .with_success(b"Hello from mock!")
            .build();

        let manifest = make_test_manifest();
        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await
            .unwrap();

        assert!(result.succeeded());
        assert_eq!(result.stdout, b"Hello from mock!");
    }

    #[tokio::test]
    async fn test_mock_runner_set_behavior() {
        let runner = MockRunner::success();
        let manifest = make_test_manifest();

        // First call succeeds
        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await
            .unwrap();
        assert!(result.succeeded());

        // Change behavior to crash
        runner.set_behavior(MockRunnerBehavior::Crash {
            message: "oops".to_string(),
        });

        // Second call crashes
        let result = runner
            .run(
                Path::new("/kernel"),
                Path::new("/drive"),
                &manifest,
                "console=ttyS0",
            )
            .await;
        assert!(matches!(result, Err(RunnerError::Crashed(_))));
    }
}
