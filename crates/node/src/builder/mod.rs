use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub mod linux;

pub mod mock;

#[derive(Debug)]
pub enum BuilderError {
    IoError(String),
    FormatError(String),
}

impl Error for BuilderError {
    fn description(&self) -> &str {
        match self {
            BuilderError::IoError(_) => "I/O error",
            BuilderError::FormatError(_) => "Format error",
        }
    }
}

impl Display for BuilderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BuilderError::IoError(msg) => write!(f, "I/O error: {}", msg),
            BuilderError::FormatError(msg) => write!(f, "Format error: {}", msg),
        }
    }
}

#[async_trait]
pub trait DriveBuilder: Send + Sync {
    /// Takes raw code string, returns path to a bootable ext4 image
    async fn create_code_drive(&self, job_id: &str, content: &str)
    -> Result<PathBuf, BuilderError>;

    /// (Future) Takes list of packages, returns path to dependency drive
    async fn build_dependency_drive(
        &self,
        job_id: &str,
        packages: Vec<String>,
    ) -> Result<PathBuf, BuilderError>;
}
