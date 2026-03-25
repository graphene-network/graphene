//! VMM Runner for job execution.
//!
//! This module provides the [`VmmRunner`] trait for running jobs in MicroVMs.
//! It wraps the lower-level [`Virtualizer`] trait to provide job-specific
//! execution with timeout enforcement and output capture.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────┐
//! │              VmmRunner                         │
//! │  - Configure VM from manifest                  │
//! │  - Set boot source from kernel registry        │
//! │  - Attach execution drive                      │
//! │  - Enforce timeout                             │
//! │  - Capture stdout/stderr                       │
//! └────────────────────────────────────────────────┘
//!                     │
//!                     ▼
//! ┌────────────────────────────────────────────────┐
//! │              Virtualizer                       │
//! │  - Low-level VMM operations                    │
//! │  - Process management                          │
//! │  - API socket communication                    │
//! └────────────────────────────────────────────────┘
//! ```

pub mod mock;

use crate::ephemeral::NetworkStats;
use crate::types::JobManifest;
use crate::vmm::{FirecrackerConfig, FirecrackerVirtualizer, Virtualizer, VmmError};
use async_trait::async_trait;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

pub use mock::{MockRunner, MockRunnerBehavior, MockRunnerBuilder, MockRunnerCall};

/// Output captured from a VM execution.
#[derive(Debug, Clone)]
pub struct VmmOutput {
    /// Exit code from the unikernel (0 = success).
    /// This is extracted from the serial output or defaults to -1 if unknown.
    pub exit_code: i32,

    /// Captured stdout from the serial console.
    pub stdout: Vec<u8>,

    /// Captured stderr from the serial console.
    /// Note: In unikernels, stdout and stderr are often mixed on the serial console.
    pub stderr: Vec<u8>,

    /// Total execution duration from start to completion.
    pub duration: Duration,

    /// True if the VM was terminated due to timeout.
    pub timed_out: bool,

    /// Network traffic statistics (egress and ingress bytes/packets).
    /// Captured from nftables counters before VM teardown.
    pub network_stats: NetworkStats,
}

impl VmmOutput {
    /// Creates a new VmmOutput with the given values.
    pub fn new(
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        duration: Duration,
        timed_out: bool,
    ) -> Self {
        Self {
            exit_code,
            stdout,
            stderr,
            duration,
            timed_out,
            network_stats: NetworkStats::default(),
        }
    }

    /// Creates a new VmmOutput with network statistics.
    pub fn with_network_stats(
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        duration: Duration,
        timed_out: bool,
        network_stats: NetworkStats,
    ) -> Self {
        Self {
            exit_code,
            stdout,
            stderr,
            duration,
            timed_out,
            network_stats,
        }
    }

    /// Returns true if the execution succeeded (exit code 0 and no timeout).
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0 && !self.timed_out
    }
}

/// Errors that can occur during VM execution.
#[derive(Debug, Error)]
pub enum RunnerError {
    /// Failed to configure the VM.
    #[error("configuration failed: {0}")]
    ConfigurationFailed(String),

    /// Failed to set the boot source.
    #[error("boot source setup failed: {0}")]
    BootSourceFailed(String),

    /// Failed to attach a drive.
    #[error("drive attachment failed: {0}")]
    DriveAttachFailed(String),

    /// Failed to start the VM.
    #[error("VM start failed: {0}")]
    StartFailed(String),

    /// VM execution timed out.
    #[error("execution timed out after {0:?}")]
    Timeout(Duration),

    /// VM crashed during execution.
    #[error("VM crashed: {0}")]
    Crashed(String),

    /// Failed to read serial output.
    #[error("output capture failed: {0}")]
    OutputCaptureFailed(String),

    /// Kernel not found in registry.
    #[error("kernel not found: {0}")]
    KernelNotFound(String),

    /// I/O error during execution.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<VmmError> for RunnerError {
    fn from(err: VmmError) -> Self {
        match err {
            VmmError::ConfigError(msg) => RunnerError::ConfigurationFailed(msg),
            VmmError::BootError(msg) => RunnerError::StartFailed(msg),
            VmmError::TimeoutError(_msg) => {
                // Parse duration from message if possible, otherwise use default
                RunnerError::Timeout(Duration::from_secs(300))
            }
            VmmError::Crash(msg) => RunnerError::Crashed(msg),
            VmmError::IoError(e) => RunnerError::IoError(e),
            other => RunnerError::Crashed(other.to_string()),
        }
    }
}

/// Trait for running jobs in MicroVMs with timeout and output capture.
///
/// This trait provides a higher-level abstraction over the [`Virtualizer`] trait,
/// handling job-specific concerns like:
/// - Configuring VM resources from the job manifest
/// - Setting up the kernel and boot arguments
/// - Attaching the execution drive
/// - Enforcing execution timeout
/// - Capturing stdout/stderr from the serial console
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow concurrent job execution.
#[async_trait]
pub trait VmmRunner: Send + Sync {
    /// Run a job in a MicroVM and return the captured output.
    ///
    /// # Arguments
    ///
    /// * `kernel_path` - Path to the unikernel binary
    /// * `drive_path` - Path to the ext4 drive image containing code and input
    /// * `manifest` - Job manifest with resource requirements and timeout
    /// * `boot_args` - Boot arguments to pass to the kernel
    ///
    /// # Returns
    ///
    /// * `Ok(VmmOutput)` - Execution completed (check exit_code for success)
    /// * `Err(RunnerError)` - Execution failed due to infrastructure error
    ///
    /// # Timeout
    ///
    /// The execution is bounded by `manifest.timeout_ms`. If the VM doesn't
    /// complete within this time, it will be forcefully terminated and
    /// `VmmOutput::timed_out` will be set to true.
    async fn run(
        &self,
        kernel_path: &Path,
        drive_path: &Path,
        manifest: &JobManifest,
        boot_args: &str,
    ) -> Result<VmmOutput, RunnerError>;
}

#[async_trait]
impl VmmRunner for Arc<dyn VmmRunner> {
    async fn run(
        &self,
        kernel_path: &Path,
        drive_path: &Path,
        manifest: &JobManifest,
        boot_args: &str,
    ) -> Result<VmmOutput, RunnerError> {
        (**self)
            .run(kernel_path, drive_path, manifest, boot_args)
            .await
    }
}

/// Configuration for the Firecracker-based VMM runner.
#[derive(Debug, Clone)]
pub struct FirecrackerRunnerConfig {
    /// Path to the firecracker binary.
    pub firecracker_bin: PathBuf,

    /// Directory for runtime files (sockets, logs).
    pub runtime_dir: PathBuf,

    /// Timeout for graceful shutdown before force kill.
    pub shutdown_timeout: Duration,
}

impl Default for FirecrackerRunnerConfig {
    fn default() -> Self {
        Self {
            firecracker_bin: PathBuf::from("firecracker"),
            runtime_dir: PathBuf::from("/tmp"),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

impl FirecrackerRunnerConfig {
    /// Creates a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the path to the firecracker binary.
    pub fn with_firecracker_bin(mut self, path: impl Into<PathBuf>) -> Self {
        self.firecracker_bin = path.into();
        self
    }

    /// Sets the runtime directory for sockets and logs.
    pub fn with_runtime_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.runtime_dir = path.into();
        self
    }

    /// Sets the shutdown timeout.
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }
}

/// Firecracker-based implementation of [`VmmRunner`].
///
/// This runner creates a new Firecracker MicroVM for each job execution,
/// configures it according to the job manifest, and captures the serial
/// console output.
type VirtualizerFactory = Arc<
    dyn Fn(
            FirecrackerConfig,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn Virtualizer>, VmmError>> + Send>>
        + Send
        + Sync,
>;

pub struct FirecrackerRunner {
    config: FirecrackerRunnerConfig,
    factory: VirtualizerFactory,
}

impl FirecrackerRunner {
    /// Creates a new Firecracker runner with the given configuration.
    pub fn new(config: FirecrackerRunnerConfig) -> Self {
        Self::with_virtualizer_factory(config, |config| async move {
            FirecrackerVirtualizer::new(config).await
        })
    }

    /// Creates a new Firecracker runner with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(FirecrackerRunnerConfig::default())
    }

    /// Creates a new Firecracker runner with a custom virtualizer factory.
    pub fn with_virtualizer_factory<F, Fut, V>(config: FirecrackerRunnerConfig, factory: F) -> Self
    where
        F: Fn(FirecrackerConfig) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<V, VmmError>> + Send + 'static,
        V: Virtualizer + 'static,
    {
        let factory: VirtualizerFactory = Arc::new(move |config: FirecrackerConfig| {
            let fut = factory(config);
            Box::pin(async move {
                let vmm = fut.await?;
                Ok(Box::new(vmm) as Box<dyn Virtualizer>)
            })
        });

        Self { config, factory }
    }

    /// Reads the serial log file and returns its contents.
    async fn read_serial_log(&self, log_path: &Path) -> Result<Vec<u8>, RunnerError> {
        if !log_path.exists() {
            debug!("Serial log file does not exist: {:?}", log_path);
            return Ok(Vec::new());
        }

        let mut file = fs::File::open(log_path).await?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await?;
        Ok(contents)
    }

    /// Extracts the exit code from serial output.
    ///
    /// Looks for a line containing "EXIT_CODE: N" at the end of output.
    /// This is a convention for unikernels to report their exit status.
    fn extract_exit_code(&self, output: &[u8]) -> i32 {
        // Convert to string, looking for exit code marker
        let output_str = String::from_utf8_lossy(output);

        // Look for EXIT_CODE: N pattern (convention for unikernel exit reporting)
        for line in output_str.lines().rev() {
            if let Some(code_str) = line.strip_prefix("EXIT_CODE: ") {
                if let Ok(code) = code_str.trim().parse::<i32>() {
                    return code;
                }
            }
            // Also check for OpenCapsule-specific format
            if let Some(code_str) = line.strip_prefix("OPENCAPSULE_EXIT: ") {
                if let Ok(code) = code_str.trim().parse::<i32>() {
                    return code;
                }
            }
        }

        // Default: if VM exited normally without crash, assume success
        // The caller should check VmmOutput::timed_out for timeout cases
        0
    }

    /// Separates stdout and stderr from serial output.
    ///
    /// In unikernels, stdout and stderr are typically interleaved on the serial
    /// console. This function attempts to separate them based on stream markers.
    fn separate_streams(&self, output: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let output_str = String::from_utf8_lossy(output);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        // Simple heuristic: lines starting with [STDERR] go to stderr
        // Everything else goes to stdout
        for line in output_str.lines() {
            if line.starts_with("[STDERR]") || line.starts_with("ERROR:") {
                stderr.extend_from_slice(line.as_bytes());
                stderr.push(b'\n');
            } else if !line.starts_with("EXIT_CODE:") && !line.starts_with("OPENCAPSULE_EXIT:") {
                stdout.extend_from_slice(line.as_bytes());
                stdout.push(b'\n');
            }
        }

        (stdout, stderr)
    }
}

#[async_trait]
impl VmmRunner for FirecrackerRunner {
    #[instrument(skip(self, manifest), fields(timeout_ms = manifest.timeout_ms))]
    async fn run(
        &self,
        kernel_path: &Path,
        drive_path: &Path,
        manifest: &JobManifest,
        boot_args: &str,
    ) -> Result<VmmOutput, RunnerError> {
        let instance_id = Uuid::new_v4().to_string();
        let log_path = self
            .config
            .runtime_dir
            .join(format!("fc-{}.log", instance_id));
        let serial_path = self
            .config
            .runtime_dir
            .join(format!("fc-{}.serial.log", instance_id));

        // Ensure runtime and log directories exist.
        if let Err(e) = std::fs::create_dir_all(&self.config.runtime_dir) {
            return Err(RunnerError::ConfigurationFailed(format!(
                "Failed to create runtime dir {}: {}",
                self.config.runtime_dir.display(),
                e
            )));
        }
        if let Some(parent) = log_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(RunnerError::ConfigurationFailed(format!(
                    "Failed to create log dir {}: {}",
                    parent.display(),
                    e
                )));
            }
        }
        // Touch the log file to ensure it exists (Firecracker fails if the path is missing).
        if let Err(e) = std::fs::File::create(&log_path) {
            return Err(RunnerError::ConfigurationFailed(format!(
                "Failed to create log file {}: {}",
                log_path.display(),
                e
            )));
        }
        if let Err(e) = std::fs::File::create(&serial_path) {
            return Err(RunnerError::ConfigurationFailed(format!(
                "Failed to create serial log file {}: {}",
                serial_path.display(),
                e
            )));
        }

        info!(
            "Starting VM execution: {} vCPUs, {} MB RAM, timeout {}ms",
            manifest.vcpu, manifest.memory_mb, manifest.timeout_ms
        );

        // Configure Firecracker
        let fc_config = FirecrackerConfig::new()
            .with_instance_id(&instance_id)
            .with_runtime_dir(&self.config.runtime_dir)
            .with_log_path(&log_path)
            .with_serial_path(&serial_path)
            .with_shutdown_timeout(self.config.shutdown_timeout)
            .with_execution_timeout(Duration::from_millis(manifest.timeout_ms));

        // Create the virtualizer
        let mut vmm = (self.factory)(fc_config)
            .await
            .map_err(|e| RunnerError::ConfigurationFailed(e.to_string()))?;

        // Configure resources
        // Note: memory_mb is u32 in manifest but u16 in Virtualizer interface
        let mem_mib = manifest.memory_mb.min(65535) as u16;
        vmm.configure(manifest.vcpu, mem_mib)
            .await
            .map_err(|e| RunnerError::ConfigurationFailed(e.to_string()))?;

        // Initrd is the only supported rootfs path with Unikraft + Firecracker.
        // Always pass the execution image as an initrd.
        let initrd_path = Some(drive_path.to_path_buf());

        // Set boot source (optionally with initrd)
        vmm.set_boot_source(
            kernel_path.to_path_buf(),
            boot_args.to_string(),
            initrd_path,
        )
        .await
        .map_err(|e| RunnerError::BootSourceFailed(e.to_string()))?;

        // Initrd-only: do not attach a root drive.

        // Start execution with timeout
        let start_time = Instant::now();
        let execution_timeout = Duration::from_millis(manifest.timeout_ms);

        // Start the VM
        vmm.start()
            .await
            .map_err(|e| RunnerError::StartFailed(e.to_string()))?;

        // Wait for completion with timeout
        let (timed_out, wait_result) = match timeout(execution_timeout, vmm.wait()).await {
            Ok(result) => (false, result),
            Err(_) => {
                warn!("VM execution timed out after {:?}", execution_timeout);
                // Force shutdown
                if let Err(e) = vmm.shutdown().await {
                    error!("Failed to shutdown VM after timeout: {}", e);
                }
                (true, Ok(()))
            }
        };

        let duration = start_time.elapsed();

        // Read serial output
        let serial_output = self
            .read_serial_log(&serial_path)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to read serial log: {}", e);
                Vec::new()
            });

        let log_to_stdout = std::env::var("OPENCAPSULE_SERIAL_LOG_STDOUT")
            .ok()
            .map(|value| value.to_ascii_lowercase())
            .and_then(|value| match value.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            })
            .unwrap_or(false);

        if log_to_stdout && !serial_output.is_empty() {
            println!(
                "--- OpenCapsule serial log ({} bytes) ---\n{}",
                serial_output.len(),
                String::from_utf8_lossy(&serial_output)
            );
        }

        let capture_path = std::env::var("OPENCAPSULE_SERIAL_LOG_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty());

        if let Some(dest) = capture_path.as_deref() {
            if let Err(e) = fs::copy(&serial_path, dest).await {
                warn!(error = %e, log_path = %serial_path.display(), dest, "Failed to copy serial log");
            } else {
                info!(log_path = %serial_path.display(), dest, "Copied serial log");
            }
        }

        let keep_serial_log = std::env::var("OPENCAPSULE_KEEP_SERIAL_LOG")
            .ok()
            .map(|value| value.to_ascii_lowercase())
            .and_then(|value| match value.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            })
            .unwrap_or(true);

        // Clean up log file unless we are keeping it for debugging
        if serial_path.exists() {
            if keep_serial_log {
                info!(log_path = %serial_path.display(), "Keeping serial log");
            } else if let Err(e) = fs::remove_file(&serial_path).await {
                debug!("Failed to remove serial log file: {}", e);
            }
        }
        if log_path.exists() {
            if let Err(e) = fs::remove_file(&log_path).await {
                debug!("Failed to remove firecracker log file: {}", e);
            }
        }

        // Check for VM crash
        if let Err(_e) = wait_result {
            if !timed_out {
                let (stdout, stderr) = self.separate_streams(&serial_output);
                return Ok(VmmOutput::new(-1, stdout, stderr, duration, false));
            }
        }

        // Extract exit code and separate streams
        let exit_code = if timed_out {
            -1
        } else {
            self.extract_exit_code(&serial_output)
        };

        let (stdout, stderr) = self.separate_streams(&serial_output);

        info!(
            "VM execution completed: exit_code={}, duration={:?}, timed_out={}",
            exit_code, duration, timed_out
        );

        Ok(VmmOutput::new(
            exit_code, stdout, stderr, duration, timed_out,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmm_output_succeeded() {
        let output = VmmOutput::new(0, vec![], vec![], Duration::from_secs(1), false);
        assert!(output.succeeded());

        let output = VmmOutput::new(1, vec![], vec![], Duration::from_secs(1), false);
        assert!(!output.succeeded());

        let output = VmmOutput::new(0, vec![], vec![], Duration::from_secs(1), true);
        assert!(!output.succeeded());
    }

    #[test]
    fn test_runner_config_builder() {
        let config = FirecrackerRunnerConfig::new()
            .with_firecracker_bin("/usr/bin/firecracker")
            .with_runtime_dir("/var/run/opencapsule")
            .with_shutdown_timeout(Duration::from_secs(10));

        assert_eq!(
            config.firecracker_bin,
            PathBuf::from("/usr/bin/firecracker")
        );
        assert_eq!(config.runtime_dir, PathBuf::from("/var/run/opencapsule"));
        assert_eq!(config.shutdown_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_extract_exit_code() {
        let runner = FirecrackerRunner::with_defaults();

        // Standard format
        let output = b"Some output\nEXIT_CODE: 0\n";
        assert_eq!(runner.extract_exit_code(output), 0);

        let output = b"Error occurred\nEXIT_CODE: 1\n";
        assert_eq!(runner.extract_exit_code(output), 1);

        // OpenCapsule format
        let output = b"Job done\nOPENCAPSULE_EXIT: 42\n";
        assert_eq!(runner.extract_exit_code(output), 42);

        // No exit code (defaults to 0)
        let output = b"Just some output\n";
        assert_eq!(runner.extract_exit_code(output), 0);
    }

    #[test]
    fn test_separate_streams() {
        let runner = FirecrackerRunner::with_defaults();

        let output = b"stdout line 1\n[STDERR] error 1\nstdout line 2\nERROR: fatal\n";
        let (stdout, stderr) = runner.separate_streams(output);

        let stdout_str = String::from_utf8_lossy(&stdout);
        let stderr_str = String::from_utf8_lossy(&stderr);

        assert!(stdout_str.contains("stdout line 1"));
        assert!(stdout_str.contains("stdout line 2"));
        assert!(stderr_str.contains("[STDERR] error 1"));
        assert!(stderr_str.contains("ERROR: fatal"));
    }

    #[test]
    fn test_runner_error_from_vmm_error() {
        let err: RunnerError = VmmError::ConfigError("bad config".to_string()).into();
        assert!(matches!(err, RunnerError::ConfigurationFailed(_)));

        let err: RunnerError = VmmError::BootError("boot failed".to_string()).into();
        assert!(matches!(err, RunnerError::StartFailed(_)));

        let err: RunnerError = VmmError::Crash("kernel panic".to_string()).into();
        assert!(matches!(err, RunnerError::Crashed(_)));
    }
}
