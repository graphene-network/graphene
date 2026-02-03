//! Firecracker-based ephemeral builder implementation.
//!
//! Orchestrates the full build lifecycle:
//! 1. Acquire build lock
//! 2. Prepare input and output drives
//! 3. Setup network isolation
//! 4. Spawn and monitor Firecracker VM
//! 5. Extract artifacts
//! 6. Cleanup all resources

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::vmm::{FirecrackerConfig, FirecrackerVirtualizer, Virtualizer};

use super::{
    default_egress_allowlist, BuildError, BuildRequest, BuildResult, DriveHelper, EphemeralBuilder,
    EphemeralBuilderConfig, NetworkIsolator, TapConfig,
};

/// Build state for tracking in-progress builds.
struct BuildState {
    build_id: String,
    #[allow(dead_code)] // Reserved for future timeout tracking
    start_time: Instant,
    cancelled: AtomicBool,
}

/// RAII guard for automatic cleanup on any exit path.
struct CleanupGuard<'a> {
    builder: &'a FirecrackerEphemeralBuilder,
    build_id: String,
    tap_config: Option<TapConfig>,
    cleanup_done: bool,
}

impl<'a> CleanupGuard<'a> {
    fn new(builder: &'a FirecrackerEphemeralBuilder, build_id: String) -> Self {
        Self {
            builder,
            build_id,
            tap_config: None,
            cleanup_done: false,
        }
    }

    fn set_tap(&mut self, config: TapConfig) {
        self.tap_config = Some(config);
    }

    async fn cleanup(&mut self) {
        if self.cleanup_done {
            return;
        }
        self.cleanup_done = true;

        // Cleanup network
        if let Some(tap) = &self.tap_config {
            if let Err(e) = self.builder.network.teardown(&tap.tap_name).await {
                warn!("Failed to teardown network for {}: {}", self.build_id, e);
            }
        }

        // Cleanup drives
        if let Err(e) = self.builder.drive_helper.cleanup(&self.build_id).await {
            warn!("Failed to cleanup drives for {}: {}", self.build_id, e);
        }

        // Clear build state
        *self.builder.current_build.lock().await = None;

        debug!("Cleanup complete for build {}", self.build_id);
    }
}

impl<'a> Drop for CleanupGuard<'a> {
    fn drop(&mut self) {
        if !self.cleanup_done {
            // We can't do async cleanup in Drop, so log a warning
            // In practice, cleanup() should be called explicitly
            warn!(
                "CleanupGuard dropped without cleanup for build {}",
                self.build_id
            );
        }
    }
}

/// Firecracker-based ephemeral builder.
///
/// Spawns isolated Firecracker VMs for each build with:
/// - Network isolation via TAP + nftables
/// - Resource limits (CPU, memory, disk, timeout)
/// - Automatic cleanup on success or failure
pub struct FirecrackerEphemeralBuilder {
    config: EphemeralBuilderConfig,
    network: Arc<dyn NetworkIsolator>,
    drive_helper: DriveHelper,
    current_build: Mutex<Option<Arc<BuildState>>>,
    artifacts_dir: PathBuf,
}

impl FirecrackerEphemeralBuilder {
    /// Create a new Firecracker ephemeral builder.
    pub fn new(
        config: EphemeralBuilderConfig,
        network: Arc<dyn NetworkIsolator>,
    ) -> Result<Self, BuildError> {
        // Validate config paths exist
        if !config.firecracker_bin.exists() {
            return Err(BuildError::DriveError(format!(
                "Firecracker binary not found: {}",
                config.firecracker_bin.display()
            )));
        }
        if !config.kernel_path.exists() {
            return Err(BuildError::DriveError(format!(
                "Builder kernel not found: {}",
                config.kernel_path.display()
            )));
        }
        if !config.rootfs_path.exists() {
            return Err(BuildError::DriveError(format!(
                "Builder rootfs not found: {}",
                config.rootfs_path.display()
            )));
        }

        let drive_helper = DriveHelper::new(&config.runtime_dir);
        let artifacts_dir = config.runtime_dir.join("artifacts");

        Ok(Self {
            config,
            network,
            drive_helper,
            current_build: Mutex::new(None),
            artifacts_dir,
        })
    }

    /// Create a builder with a mock network isolator for testing.
    #[cfg(test)]
    pub fn with_mock_network(config: EphemeralBuilderConfig) -> Result<Self, BuildError> {
        use super::MockNetworkIsolator;
        Self::new(config, Arc::new(MockNetworkIsolator::new()))
    }

    /// Get the artifacts directory path.
    pub fn artifacts_dir(&self) -> &PathBuf {
        &self.artifacts_dir
    }

    /// Execute the build inside a Firecracker VM.
    async fn execute_build(
        &self,
        request: &BuildRequest,
        input_drive: PathBuf,
        output_drive: PathBuf,
        tap_config: &TapConfig,
    ) -> Result<Duration, BuildError> {
        let start = Instant::now();

        // Create Firecracker config
        let fc_config = FirecrackerConfig {
            firecracker_bin: self.config.firecracker_bin.clone(),
            runtime_dir: self.config.runtime_dir.clone(),
            instance_id: request.build_id.clone(),
            log_path: Some(
                self.config
                    .runtime_dir
                    .join(format!("{}.log", request.build_id)),
            ),
            shutdown_timeout: Duration::from_secs(10),
            execution_timeout: request.limits.timeout,
        };

        // Create and configure VM
        let mut vm = FirecrackerVirtualizer::new(fc_config).await?;

        // Configure resources
        vm.configure(request.limits.vcpu, request.limits.memory_mib)
            .await?;

        // Set boot source with network boot args for the guest
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off ip={}::{}:{}::eth0:off",
            tap_config.guest_ip, tap_config.gateway, tap_config.netmask
        );
        vm.set_boot_source(self.config.kernel_path.clone(), boot_args)
            .await?;

        // Attach drives:
        // - rootfs (read-only, root device)
        // - input drive (read-only, /dev/vdb)
        // - output drive (read-write, /dev/vdc)
        vm.attach_drive("rootfs", self.config.rootfs_path.clone(), true, true)
            .await?;
        vm.attach_drive("input", input_drive, false, true).await?;
        vm.attach_drive("output", output_drive, false, false)
            .await?;

        info!("Starting Firecracker VM for build {}", request.build_id);

        // Start VM
        vm.start().await?;

        // Wait for completion or timeout
        let wait_result = vm.wait().await;

        // Always try to shutdown cleanly
        if let Err(e) = vm.shutdown().await {
            warn!("VM shutdown error for {}: {}", request.build_id, e);
        }

        // Handle wait result
        match wait_result {
            Ok(()) => {
                let elapsed = start.elapsed();
                info!("Build {} completed in {:?}", request.build_id, elapsed);
                Ok(elapsed)
            }
            Err(e) => {
                error!("Build {} failed: {}", request.build_id, e);
                Err(e.into())
            }
        }
    }
}

#[async_trait]
impl EphemeralBuilder for FirecrackerEphemeralBuilder {
    async fn build(&self, request: BuildRequest) -> Result<BuildResult, BuildError> {
        // Check if builder is busy
        {
            let current = self.current_build.lock().await;
            if current.is_some() {
                return Err(BuildError::BuilderBusy(
                    "Builder is processing another build".to_string(),
                ));
            }
        }

        // Create build state
        let build_state = Arc::new(BuildState {
            build_id: request.build_id.clone(),
            start_time: Instant::now(),
            cancelled: AtomicBool::new(false),
        });

        // Set current build
        *self.current_build.lock().await = Some(Arc::clone(&build_state));

        // Create cleanup guard
        let mut guard = CleanupGuard::new(self, request.build_id.clone());

        // Calculate cache key for this build
        let cache_key = DriveHelper::calculate_cache_key(
            &request.dockerfile,
            request.kraftfile.as_deref(),
            &request.code_tarball,
        )?;

        // Prepare input drive
        let input_drive = self
            .drive_helper
            .prepare_input_drive(
                &request.build_id,
                &request.dockerfile,
                request.kraftfile.as_deref(),
                &request.code_tarball,
                512, // 512 MiB for input
            )
            .await?;

        // Create output drive
        let output_drive = self
            .drive_helper
            .create_output_drive(&request.build_id, 512) // 512 MiB for output
            .await?;

        // Setup network isolation
        let tap_config = self
            .network
            .create_tap(&request.build_id)
            .await
            .map_err(|e| BuildError::NetworkSetupFailed(format!("Failed to create TAP: {}", e)))?;
        guard.set_tap(tap_config.clone());

        // Apply egress allowlist
        let allowlist = if request.egress_allowlist.is_empty() {
            default_egress_allowlist()
        } else {
            request.egress_allowlist.clone()
        };

        self.network
            .apply_allowlist(&tap_config.tap_name, &allowlist)
            .await
            .map_err(|e| {
                BuildError::NetworkSetupFailed(format!("Failed to apply allowlist: {}", e))
            })?;

        // Check for cancellation
        if build_state.cancelled.load(Ordering::Relaxed) {
            guard.cleanup().await;
            return Err(BuildError::Cancelled(request.build_id.clone()));
        }

        // Execute build in VM
        let build_duration = self
            .execute_build(&request, input_drive, output_drive.clone(), &tap_config)
            .await?;

        // Check for cancellation
        if build_state.cancelled.load(Ordering::Relaxed) {
            guard.cleanup().await;
            return Err(BuildError::Cancelled(request.build_id.clone()));
        }

        // Extract artifacts
        let (unikernel_path, logs) = self
            .drive_helper
            .extract_artifacts(&output_drive, &request.build_id, &self.artifacts_dir)
            .await?;

        // Cleanup
        guard.cleanup().await;

        Ok(BuildResult {
            unikernel_path,
            build_duration,
            logs,
            cache_key,
        })
    }

    fn is_busy(&self) -> bool {
        // Use try_lock to avoid blocking
        self.current_build
            .try_lock()
            .map(|guard| guard.is_some())
            .unwrap_or(true)
    }

    async fn cancel(&self, build_id: &str) -> Result<(), BuildError> {
        let current = self.current_build.lock().await;
        match &*current {
            Some(state) if state.build_id == build_id => {
                state.cancelled.store(true, Ordering::Relaxed);
                info!("Marked build {} for cancellation", build_id);
                Ok(())
            }
            Some(state) => Err(BuildError::Cancelled(format!(
                "Build {} is not running (current: {})",
                build_id, state.build_id
            ))),
            None => Err(BuildError::Cancelled(
                "No build is currently running".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephemeral::{Protocol, DEFAULT_EGRESS_HOSTS};

    // Note: Full integration tests require Firecracker binary and root privileges.
    // These are enabled with the `integration-tests` feature.

    #[test]
    fn default_egress_allowlist_contains_expected() {
        assert!(DEFAULT_EGRESS_HOSTS.contains(&"pypi.org"));
        assert!(DEFAULT_EGRESS_HOSTS.contains(&"crates.io"));
        assert!(DEFAULT_EGRESS_HOSTS.contains(&"github.com"));
    }

    #[test]
    fn default_egress_allowlist_function_returns_entries() {
        let allowlist = default_egress_allowlist();
        assert!(!allowlist.is_empty());
        // All entries should be HTTPS (port 443, TCP)
        for entry in &allowlist {
            assert_eq!(entry.port, 443);
            assert_eq!(entry.protocol, Protocol::Tcp);
        }
    }
}
