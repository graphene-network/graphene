use super::{Virtualizer, VmmError};
use async_trait::async_trait;
use firecracker_rs_sdk::{
    firecracker::FirecrackerOption,
    models::{BootSource, Drive, MachineConfiguration},
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration for a Firecracker MicroVM instance.
#[derive(Debug, Clone)]
pub struct FirecrackerConfig {
    /// Path to the firecracker binary. Defaults to "firecracker" (looks in PATH).
    pub firecracker_bin: PathBuf,
    /// Directory for runtime files (sockets, logs). Defaults to "/tmp".
    pub runtime_dir: PathBuf,
    /// Unique instance identifier. Auto-generated if not provided.
    pub instance_id: String,
    /// Optional path for serial console output.
    pub log_path: Option<PathBuf>,
    /// Optional path for capturing VM stdout (serial console).
    pub serial_path: Option<PathBuf>,
    /// Timeout for graceful shutdown before force kill. Defaults to 5 seconds.
    pub shutdown_timeout: Duration,
    /// Maximum execution time before timeout. Defaults to 300 seconds.
    pub execution_timeout: Duration,
}

impl Default for FirecrackerConfig {
    fn default() -> Self {
        Self {
            firecracker_bin: PathBuf::from("firecracker"),
            runtime_dir: PathBuf::from("/tmp"),
            instance_id: Uuid::new_v4().to_string(),
            log_path: None,
            serial_path: None,
            shutdown_timeout: Duration::from_secs(5),
            execution_timeout: Duration::from_secs(300),
        }
    }
}

impl FirecrackerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_instance_id(mut self, id: impl Into<String>) -> Self {
        self.instance_id = id.into();
        self
    }

    pub fn with_runtime_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.runtime_dir = dir.into();
        self
    }

    pub fn with_log_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.log_path = Some(path.into());
        self
    }

    pub fn with_serial_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.serial_path = Some(path.into());
        self
    }

    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    pub fn with_execution_timeout(mut self, timeout: Duration) -> Self {
        self.execution_timeout = timeout;
        self
    }
}

/// VM lifecycle states for tracking operation sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmState {
    /// Process spawned, API socket ready
    Created,
    /// Machine configuration set (vCPU, memory)
    Configured,
    /// Boot source and drives attached, ready to boot
    BootReady,
    /// VM is running
    Running,
    /// Graceful shutdown in progress
    ShuttingDown,
    /// VM has terminated
    Terminated,
}

/// Firecracker-based Virtualizer implementation for MicroVM lifecycle management.
pub struct FirecrackerVirtualizer {
    config: FirecrackerConfig,
    instance: Option<firecracker_rs_sdk::instance::Instance>,
    socket_path: PathBuf,
    state: VmState,
    boot_source_set: bool,
    initrd_set: bool,
    drives_attached: u32,
}

impl FirecrackerVirtualizer {
    /// Create a new Firecracker virtualizer and spawn the VMM process.
    ///
    /// This will:
    /// 1. Generate a unique socket path
    /// 2. Clean up any stale socket file
    /// 3. Spawn the firecracker process
    /// 4. Wait for the API socket to become available
    pub async fn new(config: FirecrackerConfig) -> Result<Self, VmmError> {
        let socket_path = config
            .runtime_dir
            .join(format!("firecracker-{}.sock", config.instance_id));

        // Clean up stale socket if it exists
        if socket_path.exists() {
            debug!("Removing stale socket: {:?}", socket_path);
            std::fs::remove_file(&socket_path)?;
        }

        info!("Starting Firecracker VMM with socket: {:?}", socket_path);

        // Build the Firecracker instance
        let mut option = FirecrackerOption::new(&config.firecracker_bin);
        option.api_sock(&socket_path);

        // Add log path if configured
        if let Some(ref log_path) = config.log_path {
            option.log_path(Some(log_path));
        }
        // Capture VM stdout to serial path if configured.
        if let Some(ref serial_path) = config.serial_path {
            option.stdout(serial_path);
        }

        let mut instance = option.build().map_err(|e| {
            VmmError::ProcessSpawnError(format!("Failed to build Firecracker instance: {}", e))
        })?;

        // Start the VMM process
        instance.start_vmm().await.map_err(|e| {
            VmmError::ProcessSpawnError(format!("Failed to start VMM process: {}", e))
        })?;

        debug!("API socket ready: {:?}", socket_path);

        Ok(Self {
            config,
            instance: Some(instance),
            socket_path,
            state: VmState::Created,
            boot_source_set: false,
            initrd_set: false,
            drives_attached: 0,
        })
    }

    /// Get the current VM state.
    pub fn state(&self) -> &VmState {
        &self.state
    }

    /// Get the API socket path.
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get the instance ID.
    pub fn instance_id(&self) -> &str {
        &self.config.instance_id
    }

    fn instance_mut(&mut self) -> Result<&mut firecracker_rs_sdk::instance::Instance, VmmError> {
        self.instance
            .as_mut()
            .ok_or_else(|| VmmError::RuntimeError("VMM instance not initialized".to_string()))
    }

    fn validate_state(&self, expected: &[VmState], operation: &str) -> Result<(), VmmError> {
        if !expected.contains(&self.state) {
            return Err(VmmError::RuntimeError(format!(
                "Invalid state for {}: expected one of {:?}, got {:?}",
                operation, expected, self.state
            )));
        }
        Ok(())
    }

    fn update_boot_ready_state(&mut self) {
        if self.boot_source_set && (self.drives_attached > 0 || self.initrd_set) {
            self.state = VmState::BootReady;
        }
    }

    async fn force_kill(&mut self) -> Result<(), VmmError> {
        warn!("Force killing VMM instance: {}", self.config.instance_id);

        if let Some(ref instance) = self.instance {
            if let Some(pid) = instance.firecracker_pid() {
                // Send SIGKILL to the firecracker process
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }

        self.cleanup();
        Ok(())
    }

    fn cleanup(&mut self) {
        // Remove socket file if it exists
        if self.socket_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.socket_path) {
                warn!("Failed to remove socket file: {}", e);
            }
        }

        self.state = VmState::Terminated;
    }

    /// Wait for the VM process to exit, with a timeout.
    async fn wait_for_exit(&self, wait_timeout: Duration) -> Result<bool, VmmError> {
        let instance = self
            .instance
            .as_ref()
            .ok_or_else(|| VmmError::RuntimeError("VMM instance not initialized".to_string()))?;

        let pid = instance
            .firecracker_pid()
            .ok_or_else(|| VmmError::RuntimeError("Firecracker process not started".to_string()))?;

        let poll_interval = Duration::from_millis(100);
        let start = std::time::Instant::now();

        while start.elapsed() < wait_timeout {
            #[cfg(target_os = "linux")]
            {
                let cmdline_path = format!("/proc/{}/cmdline", pid);
                match std::fs::read_to_string(&cmdline_path) {
                    Ok(cmdline) => {
                        // /proc/<pid>/cmdline is NUL-separated
                        if !cmdline.contains("firecracker") {
                            return Ok(true);
                        }
                    }
                    Err(_) => {
                        return Ok(true);
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                // Check if process is still running using kill(pid, 0)
                let result = unsafe { libc::kill(pid as i32, 0) };
                if result != 0 {
                    // Process has exited
                    return Ok(true);
                }
            }
            sleep(poll_interval).await;
        }

        // Timeout reached, process still running
        Ok(false)
    }
}

#[async_trait]
impl Virtualizer for FirecrackerVirtualizer {
    async fn configure(&mut self, vcpu: u8, mem_mib: u16) -> Result<(), VmmError> {
        self.validate_state(&[VmState::Created], "configure")?;

        // Validate resource bounds per spec
        if !(1..=16).contains(&vcpu) {
            return Err(VmmError::ConfigError(format!(
                "vCPU count must be between 1 and 16, got {}",
                vcpu
            )));
        }
        if mem_mib < 128 {
            return Err(VmmError::ConfigError(format!(
                "Memory must be at least 128 MiB, got {} MiB",
                mem_mib
            )));
        }

        info!("Configuring VM: {} vCPUs, {} MiB memory", vcpu, mem_mib);

        let machine_config = MachineConfiguration {
            vcpu_count: vcpu as isize,
            mem_size_mib: mem_mib as isize,
            cpu_template: None,
            smt: None,
            track_dirty_pages: None,
            huge_pages: None,
        };

        self.instance_mut()?
            .put_machine_configuration(&machine_config)
            .await
            .map_err(|e| VmmError::ApiError(format!("Failed to configure machine: {}", e)))?;

        self.state = VmState::Configured;
        debug!("VM configured successfully");

        Ok(())
    }

    async fn set_boot_source(
        &mut self,
        kernel_path: PathBuf,
        boot_args: String,
        initrd_path: Option<PathBuf>,
    ) -> Result<(), VmmError> {
        self.validate_state(
            &[VmState::Configured, VmState::BootReady],
            "set_boot_source",
        )?;

        info!("Setting boot source: {:?}", kernel_path);

        let boot_source = BootSource {
            kernel_image_path: kernel_path,
            boot_args: Some(boot_args),
            initrd_path: initrd_path.clone(),
        };

        self.instance_mut()?
            .put_guest_boot_source(&boot_source)
            .await
            .map_err(|e| VmmError::ApiError(format!("Failed to set boot source: {}", e)))?;

        self.boot_source_set = true;
        self.initrd_set = initrd_path.is_some();
        self.update_boot_ready_state();
        debug!("Boot source set successfully");

        Ok(())
    }

    async fn attach_drive(
        &mut self,
        drive_id: &str,
        path: PathBuf,
        is_root: bool,
        read_only: bool,
    ) -> Result<(), VmmError> {
        self.validate_state(&[VmState::Configured, VmState::BootReady], "attach_drive")?;

        info!(
            "Attaching drive '{}': {:?} (root={}, ro={})",
            drive_id, path, is_root, read_only
        );

        let drive = Drive {
            drive_id: drive_id.to_string(),
            path_on_host: path,
            is_root_device: is_root,
            is_read_only: read_only,
            partuuid: None,
            cache_type: None,
            rate_limiter: None,
            io_engine: None,
            socket: None,
        };

        self.instance_mut()?
            .put_guest_drive_by_id(&drive)
            .await
            .map_err(|e| VmmError::ApiError(format!("Failed to attach drive: {}", e)))?;

        self.drives_attached += 1;
        self.update_boot_ready_state();
        debug!("Drive '{}' attached successfully", drive_id);

        Ok(())
    }

    async fn start(&mut self) -> Result<(), VmmError> {
        self.validate_state(&[VmState::BootReady], "start")?;

        info!("Starting VM: {}", self.config.instance_id);

        self.instance_mut()?
            .start()
            .await
            .map_err(|e| VmmError::BootError(format!("Failed to start VM: {}", e)))?;

        self.state = VmState::Running;
        info!("VM started successfully");

        Ok(())
    }

    async fn wait(&mut self) -> Result<(), VmmError> {
        self.validate_state(&[VmState::Running], "wait")?;

        info!(
            "Waiting for VM to exit (timeout: {:?})",
            self.config.execution_timeout
        );

        let exited = self.wait_for_exit(self.config.execution_timeout).await?;

        if exited {
            info!("VM exited");
            self.state = VmState::Terminated;
            Ok(())
        } else {
            error!("VM execution timed out");
            self.force_kill().await?;
            Err(VmmError::TimeoutError(format!(
                "VM execution exceeded timeout of {:?}",
                self.config.execution_timeout
            )))
        }
    }

    async fn shutdown(&mut self) -> Result<(), VmmError> {
        if self.state == VmState::Terminated {
            return Ok(());
        }

        self.validate_state(&[VmState::Running, VmState::ShuttingDown], "shutdown")?;

        info!(
            "Initiating graceful shutdown for VM: {}",
            self.config.instance_id
        );
        self.state = VmState::ShuttingDown;

        // Send CtrlAltDel for graceful shutdown
        if let Err(e) = self.instance_mut()?.stop().await {
            warn!("Failed to send graceful shutdown signal: {}", e);
        }

        // Wait for graceful shutdown with timeout
        let exited = self.wait_for_exit(self.config.shutdown_timeout).await?;

        if exited {
            info!("VM shut down gracefully");
            self.cleanup();
            Ok(())
        } else {
            warn!(
                "Graceful shutdown timed out after {:?}, force killing",
                self.config.shutdown_timeout
            );
            self.force_kill().await
        }
    }
}

impl Drop for FirecrackerVirtualizer {
    fn drop(&mut self) {
        // Best-effort cleanup on drop
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = FirecrackerConfig::default();
        assert_eq!(config.firecracker_bin, PathBuf::from("firecracker"));
        assert_eq!(config.runtime_dir, PathBuf::from("/tmp"));
        assert_eq!(config.shutdown_timeout, Duration::from_secs(5));
        assert_eq!(config.execution_timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_config_builder() {
        let config = FirecrackerConfig::new()
            .with_instance_id("test-vm")
            .with_runtime_dir("/var/run")
            .with_shutdown_timeout(Duration::from_secs(10));

        assert_eq!(config.instance_id, "test-vm");
        assert_eq!(config.runtime_dir, PathBuf::from("/var/run"));
        assert_eq!(config.shutdown_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_vm_state_equality() {
        assert_eq!(VmState::Created, VmState::Created);
        assert_ne!(VmState::Created, VmState::Running);
    }
}
