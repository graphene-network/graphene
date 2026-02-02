pub mod dockerfile;
pub mod kraft;
pub mod mock;
pub mod types;

pub use dockerfile::{DockerfileParser, DockerfileValidator};
pub use kraft::{KraftBuilder, KraftConfig};
pub use mock::{MockBuildBehavior, MockKraftBuilder};
pub use types::{
    BuildJob, BuildManifest, Kraftfile, ResourceLimits, Runtime, UnikernelImage,
    ValidatedDockerfile,
};

use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;

/// Errors that can occur during unikernel build operations
#[derive(Debug)]
pub enum UnikraftError {
    /// Failed to parse Dockerfile syntax
    DockerfileParseError(String),
    /// Dockerfile contains a forbidden command
    UnsupportedCommand { command: String, reason: String },
    /// Base image is not allowed
    UnsupportedBaseImage(String),
    /// Invalid RUN command pattern
    InvalidRunCommand(String),
    /// Failed to generate or write Kraftfile
    KraftfileError(String),
    /// Build exceeded time limit
    BuildTimeout { elapsed: Duration, limit: Duration },
    /// kraft CLI returned non-zero exit code
    BuildFailed { exit_code: i32, stderr: String },
    /// Build was cancelled
    BuildCancelled,
    /// I/O error during build
    IoError(std::io::Error),
    /// Source tar extraction failed
    TarError(String),
}

impl Error for UnikraftError {
    fn description(&self) -> &str {
        match self {
            UnikraftError::DockerfileParseError(_) => "Dockerfile parse error",
            UnikraftError::UnsupportedCommand { .. } => "Unsupported Dockerfile command",
            UnikraftError::UnsupportedBaseImage(_) => "Unsupported base image",
            UnikraftError::InvalidRunCommand(_) => "Invalid RUN command",
            UnikraftError::KraftfileError(_) => "Kraftfile error",
            UnikraftError::BuildTimeout { .. } => "Build timeout",
            UnikraftError::BuildFailed { .. } => "Build failed",
            UnikraftError::BuildCancelled => "Build cancelled",
            UnikraftError::IoError(_) => "I/O error",
            UnikraftError::TarError(_) => "Tar extraction error",
        }
    }
}

impl Display for UnikraftError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UnikraftError::DockerfileParseError(msg) => {
                write!(f, "Failed to parse Dockerfile: {}", msg)
            }
            UnikraftError::UnsupportedCommand { command, reason } => {
                write!(
                    f,
                    "Unsupported Dockerfile command '{}': {}",
                    command, reason
                )
            }
            UnikraftError::UnsupportedBaseImage(image) => {
                write!(f, "Unsupported base image: {}", image)
            }
            UnikraftError::InvalidRunCommand(cmd) => {
                write!(f, "Invalid RUN command: {}", cmd)
            }
            UnikraftError::KraftfileError(msg) => {
                write!(f, "Kraftfile error: {}", msg)
            }
            UnikraftError::BuildTimeout { elapsed, limit } => {
                write!(
                    f,
                    "Build timed out after {:?} (limit: {:?})",
                    elapsed, limit
                )
            }
            UnikraftError::BuildFailed { exit_code, stderr } => {
                write!(f, "Build failed with exit code {}: {}", exit_code, stderr)
            }
            UnikraftError::BuildCancelled => write!(f, "Build was cancelled"),
            UnikraftError::IoError(err) => write!(f, "I/O error: {}", err),
            UnikraftError::TarError(msg) => write!(f, "Tar extraction error: {}", msg),
        }
    }
}

impl From<std::io::Error> for UnikraftError {
    fn from(err: std::io::Error) -> Self {
        UnikraftError::IoError(err)
    }
}

/// Trait for building unikernels from Dockerfiles
#[async_trait]
pub trait UnikernelBuilder: Send + Sync {
    /// Build a unikernel from the provided job specification
    async fn build(&self, job: &BuildJob) -> Result<UnikernelImage, UnikraftError>;

    /// Validate a Dockerfile and return parsed information
    fn validate_dockerfile(&self, dockerfile: &str) -> Result<ValidatedDockerfile, UnikraftError>;

    /// Generate a Kraftfile from a build manifest
    fn generate_kraftfile(&self, manifest: &BuildManifest, name: &str) -> Kraftfile;
}
