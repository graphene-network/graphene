//! Ephemeral Builder VM Module
//!
//! Provides isolated VM environments for building unikernels from Dockerfiles.
//! Each build spawns a fresh Firecracker MicroVM with network restrictions,
//! resource limits, and automatic cleanup.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    EphemeralBuilder                              │
//! │  ┌───────────────┐  ┌──────────────────┐  ┌─────────────────┐  │
//! │  │ NetworkIsolator│  │ FirecrackerVMM   │  │ DriveHelpers    │  │
//! │  │ (TAP + nftables)│ │ (from vmm module)│  │ (ext4 mounting) │  │
//! │  └───────────────┘  └──────────────────┘  └─────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```text
//! use monad_node::ephemeral::{
//!     BuildRequest, EphemeralBuilder, EphemeralBuilderConfig,
//!     FirecrackerEphemeralBuilder, MockNetworkIsolator,
//! };
//! use std::sync::Arc;
//!
//! let config = EphemeralBuilderConfig::default();
//! let network = Arc::new(MockNetworkIsolator::new());
//! let builder = FirecrackerEphemeralBuilder::new(config, network)?;
//!
//! let request = BuildRequest::new("build-123", "FROM python:3.12")
//!     .code_tarball("/tmp/code.tar.gz")
//!     .egress_allowlist(vec!["pypi.org".into()]);
//!
//! let result = builder.build(request).await?;
//! println!("Unikernel built: {:?}", result.unikernel_path);
//! ```

mod types;
pub use types::*;

mod builder;
pub use builder::*;

mod network;
pub use network::*;

mod drive;
pub use drive::*;

mod mock;
pub use mock::*;

use async_trait::async_trait;
use std::fmt;
use std::time::Duration;

use crate::vmm::VmmError;

/// Error types for ephemeral builder operations.
#[derive(Debug)]
pub enum BuildError {
    /// Build exceeded the configured timeout
    Timeout { elapsed: Duration, limit: Duration },
    /// Build exceeded memory limit
    OutOfMemory { requested_mib: u32, limit_mib: u16 },
    /// Output drive ran out of space
    DiskFull { used_mib: u32, limit_mib: u32 },
    /// Dockerfile parsing or execution error
    DockerfileInvalid(String),
    /// Kraftfile parsing or configuration error
    KraftfileInvalid(String),
    /// Network isolation setup failed
    NetworkSetupFailed(String),
    /// Error from the VMM subsystem
    VmmError(VmmError),
    /// Failed to extract build artifacts from output drive
    ArtifactExtractionFailed(String),
    /// Builder is currently processing another build
    BuilderBusy(String),
    /// Build was cancelled
    Cancelled(String),
    /// I/O error during build operations
    IoError(std::io::Error),
    /// Drive preparation error
    DriveError(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Timeout { elapsed, limit } => {
                write!(
                    f,
                    "Build timed out after {:?} (limit: {:?})",
                    elapsed, limit
                )
            }
            BuildError::OutOfMemory {
                requested_mib,
                limit_mib,
            } => {
                write!(
                    f,
                    "Out of memory: requested {} MiB, limit {} MiB",
                    requested_mib, limit_mib
                )
            }
            BuildError::DiskFull {
                used_mib,
                limit_mib,
            } => {
                write!(
                    f,
                    "Disk full: used {} MiB, limit {} MiB",
                    used_mib, limit_mib
                )
            }
            BuildError::DockerfileInvalid(msg) => {
                write!(f, "Invalid Dockerfile: {}", msg)
            }
            BuildError::KraftfileInvalid(msg) => {
                write!(f, "Invalid Kraftfile: {}", msg)
            }
            BuildError::NetworkSetupFailed(msg) => {
                write!(f, "Network setup failed: {}", msg)
            }
            BuildError::VmmError(e) => {
                write!(f, "VMM error: {}", e)
            }
            BuildError::ArtifactExtractionFailed(msg) => {
                write!(f, "Artifact extraction failed: {}", msg)
            }
            BuildError::BuilderBusy(msg) => {
                write!(f, "Builder busy: {}", msg)
            }
            BuildError::Cancelled(msg) => {
                write!(f, "Build cancelled: {}", msg)
            }
            BuildError::IoError(e) => {
                write!(f, "I/O error: {}", e)
            }
            BuildError::DriveError(msg) => {
                write!(f, "Drive error: {}", msg)
            }
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::IoError(e) => Some(e),
            BuildError::VmmError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BuildError {
    fn from(err: std::io::Error) -> Self {
        BuildError::IoError(err)
    }
}

impl From<VmmError> for BuildError {
    fn from(err: VmmError) -> Self {
        BuildError::VmmError(err)
    }
}

/// Error types for network isolation operations.
#[derive(Debug)]
pub enum NetworkError {
    /// Failed to create TAP device
    TapCreationFailed(String),
    /// Failed to configure IP address
    IpConfigFailed(String),
    /// Failed to apply firewall rules
    FirewallError(String),
    /// Failed to resolve hostname for allowlist
    DnsResolutionFailed(String),
    /// Failed to teardown network resources
    TeardownFailed(String),
    /// I/O error during network operations
    IoError(std::io::Error),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::TapCreationFailed(msg) => {
                write!(f, "TAP creation failed: {}", msg)
            }
            NetworkError::IpConfigFailed(msg) => {
                write!(f, "IP configuration failed: {}", msg)
            }
            NetworkError::FirewallError(msg) => {
                write!(f, "Firewall error: {}", msg)
            }
            NetworkError::DnsResolutionFailed(msg) => {
                write!(f, "DNS resolution failed: {}", msg)
            }
            NetworkError::TeardownFailed(msg) => {
                write!(f, "Network teardown failed: {}", msg)
            }
            NetworkError::IoError(e) => {
                write!(f, "I/O error: {}", e)
            }
        }
    }
}

impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NetworkError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for NetworkError {
    fn from(err: std::io::Error) -> Self {
        NetworkError::IoError(err)
    }
}

/// Trait for ephemeral build execution.
///
/// Implementations manage the lifecycle of builder VMs, including:
/// - Drive preparation (input with code, output for artifacts)
/// - Network isolation setup
/// - VM execution with resource limits
/// - Artifact extraction and cleanup
#[async_trait]
pub trait EphemeralBuilder: Send + Sync {
    /// Execute a build request and return the resulting unikernel.
    ///
    /// This method:
    /// 1. Prepares input drive with Dockerfile, Kraftfile, and code
    /// 2. Creates empty output drive for artifacts
    /// 3. Sets up network isolation with egress allowlist
    /// 4. Spawns Firecracker VM with resource limits
    /// 5. Waits for build completion or timeout
    /// 6. Extracts artifacts from output drive
    /// 7. Cleans up all resources
    async fn build(&self, request: BuildRequest) -> Result<BuildResult, BuildError>;

    /// Check if the builder is currently processing a build.
    fn is_busy(&self) -> bool;

    /// Cancel an in-progress build.
    ///
    /// Returns an error if no build with the given ID is running.
    async fn cancel(&self, build_id: &str) -> Result<(), BuildError>;
}

/// Trait for network isolation management.
///
/// Implementations handle TAP device creation, IP configuration,
/// and firewall rules to restrict VM network access.
#[async_trait]
pub trait NetworkIsolator: Send + Sync {
    /// Create a TAP device for a VM.
    ///
    /// Returns configuration needed to attach the TAP to Firecracker.
    async fn create_tap(&self, vm_id: &str) -> Result<TapConfig, NetworkError>;

    /// Apply egress allowlist firewall rules.
    ///
    /// After calling this:
    /// - Traffic to allowlisted hosts is permitted
    /// - RFC1918 addresses (10.x, 172.16.x, 192.168.x, 127.x) are blocked
    /// - All other traffic is dropped
    async fn apply_allowlist(
        &self,
        tap_name: &str,
        allowlist: &[String],
    ) -> Result<(), NetworkError>;

    /// Tear down network resources for a VM.
    ///
    /// Removes TAP device and associated firewall rules.
    async fn teardown(&self, tap_name: &str) -> Result<(), NetworkError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.timeout, Duration::from_secs(300));
        assert_eq!(limits.memory_mib, 4096);
        assert_eq!(limits.disk_mib, 10240);
        assert_eq!(limits.vcpu, 2);
    }

    #[test]
    fn resource_limits_builder() {
        let limits = ResourceLimits::new()
            .timeout(Duration::from_secs(60))
            .memory_mib(2048)
            .disk_mib(5120)
            .vcpu(4);

        assert_eq!(limits.timeout, Duration::from_secs(60));
        assert_eq!(limits.memory_mib, 2048);
        assert_eq!(limits.disk_mib, 5120);
        assert_eq!(limits.vcpu, 4);
    }

    #[test]
    fn build_request_builder() {
        let request = BuildRequest::new("test-123", "FROM alpine")
            .kraftfile("name: test")
            .code_tarball("/tmp/code.tar.gz")
            .egress_allowlist(vec!["pypi.org".into()]);

        assert_eq!(request.build_id, "test-123");
        assert_eq!(request.dockerfile, "FROM alpine");
        assert_eq!(request.kraftfile, Some("name: test".to_string()));
        assert_eq!(request.code_tarball.to_str().unwrap(), "/tmp/code.tar.gz");
        assert_eq!(request.egress_allowlist, vec!["pypi.org"]);
    }

    #[test]
    fn tap_config_for_vm() {
        let config = TapConfig::for_vm("build-abcd1234");
        assert_eq!(config.tap_name, "tap-abcd1234");
        assert_eq!(config.host_ip, "172.16.0.1");
        assert_eq!(config.guest_ip, "172.16.0.2");
    }

    #[test]
    fn build_error_display() {
        let err = BuildError::Timeout {
            elapsed: Duration::from_secs(310),
            limit: Duration::from_secs(300),
        };
        assert!(err.to_string().contains("timed out"));

        let err = BuildError::DockerfileInvalid("missing FROM".into());
        assert!(err.to_string().contains("missing FROM"));
    }

    #[test]
    fn network_error_display() {
        let err = NetworkError::TapCreationFailed("permission denied".into());
        assert!(err.to_string().contains("permission denied"));
    }
}
