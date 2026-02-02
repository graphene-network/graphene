use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug)]
pub enum VmmError {
    ConfigError(String),
    BootError(String),
    RuntimeError(String),
    Crash(String),
    ProcessSpawnError(String),
    SocketError(String),
    ApiError(String),
    TimeoutError(String),
    IoError(std::io::Error),
}

impl Error for VmmError {
    fn description(&self) -> &str {
        match self {
            VmmError::ConfigError(_) => "Configuration error",
            VmmError::BootError(_) => "Boot error",
            VmmError::RuntimeError(_) => "Runtime error",
            VmmError::Crash(_) => "Crash",
            VmmError::ProcessSpawnError(_) => "Process spawn error",
            VmmError::SocketError(_) => "Socket error",
            VmmError::ApiError(_) => "API error",
            VmmError::TimeoutError(_) => "Timeout error",
            VmmError::IoError(_) => "I/O error",
        }
    }
}

impl Display for VmmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VmmError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            VmmError::BootError(msg) => write!(f, "Boot error: {}", msg),
            VmmError::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            VmmError::Crash(msg) => write!(f, "Crash: {}", msg),
            VmmError::ProcessSpawnError(msg) => write!(f, "Process spawn error: {}", msg),
            VmmError::SocketError(msg) => write!(f, "Socket error: {}", msg),
            VmmError::ApiError(msg) => write!(f, "API error: {}", msg),
            VmmError::TimeoutError(msg) => write!(f, "Timeout error: {}", msg),
            VmmError::IoError(err) => write!(f, "I/O error: {}", err),
        }
    }
}

impl From<std::io::Error> for VmmError {
    fn from(err: std::io::Error) -> Self {
        VmmError::IoError(err)
    }
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
