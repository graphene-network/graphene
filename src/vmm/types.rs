use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug)]
pub enum VmmError {
    ConfigError(String),
    BootError(String),
    RuntimeError(String),
    Crash(String), // Simulated crash
}

#[async_trait]
pub trait Virtualizer: Send + Sync {
    /// Configure CPU and RAM
    async fn configure(&mut self, vcpu: u8, mem_mib: u16) -> Result<(), VmmError>;

    /// Set the kernel and boot arguments
    async fn set_boot_source(
        &mut self,
        kernel_path: PathBuf,
        boot_args: String,
    ) -> Result<(), VmmError>;

    /// Attach a block device (Layer 1, 2, or 3)
    async fn attach_drive(
        &mut self,
        drive_id: &str,
        path: PathBuf,
        is_root: bool,
        read_only: bool,
    ) -> Result<(), VmmError>;

    /// Boot the machine
    async fn start(&mut self) -> Result<(), VmmError>;

    /// Wait for the machine to shut down (or crash)
    async fn wait(&mut self) -> Result<(), VmmError>;

    /// Hard kill the machine
    async fn shutdown(&mut self) -> Result<(), VmmError>;
}
