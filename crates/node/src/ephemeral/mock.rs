//! Mock implementations for testing ephemeral builder functionality.
//!
//! Provides configurable mock behaviors for both EphemeralBuilder and NetworkIsolator
//! traits, enabling comprehensive unit testing without requiring actual Firecracker
//! VMs or network capabilities.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{
    BuildError, BuildRequest, BuildResult, EgressEntry, EphemeralBuilder, NetworkError,
    NetworkIsolator, TapConfig,
};

/// Configurable behaviors for MockEphemeralBuilder.
#[derive(Debug, Clone)]
pub enum MockBuildBehavior {
    /// Build succeeds after specified delay
    Success {
        delay: Duration,
        artifact_size: usize,
    },
    /// Build times out
    Timeout,
    /// Build fails with out-of-memory error
    OutOfMemory { requested_mib: u32 },
    /// Build fails with disk full error
    DiskFull { used_mib: u32 },
    /// Dockerfile is invalid
    DockerfileError(String),
    /// Network setup fails
    NetworkError,
    /// Artifact extraction fails
    ArtifactExtractionError(String),
    /// Builder is busy with another build
    Busy,
}

impl Default for MockBuildBehavior {
    fn default() -> Self {
        MockBuildBehavior::Success {
            delay: Duration::from_millis(100),
            artifact_size: 1024,
        }
    }
}

/// Spy state for tracking mock builder calls.
#[derive(Debug, Default)]
pub struct BuilderSpyState {
    /// All build requests received
    pub build_calls: Vec<BuildRequest>,
    /// Build IDs for which cancel was called
    pub cancel_calls: Vec<String>,
    /// Number of times is_busy was checked
    pub is_busy_calls: usize,
}

/// Mock implementation of EphemeralBuilder for testing.
pub struct MockEphemeralBuilder {
    behavior: Mutex<MockBuildBehavior>,
    spy: Arc<Mutex<BuilderSpyState>>,
    is_busy: Mutex<bool>,
    output_dir: PathBuf,
}

impl MockEphemeralBuilder {
    /// Create a new mock builder with default success behavior.
    pub fn new() -> Self {
        Self {
            behavior: Mutex::new(MockBuildBehavior::default()),
            spy: Arc::new(Mutex::new(BuilderSpyState::default())),
            is_busy: Mutex::new(false),
            output_dir: std::env::temp_dir().join("mock-ephemeral-builder"),
        }
    }

    /// Create a new mock builder with specified behavior.
    pub fn with_behavior(behavior: MockBuildBehavior) -> Self {
        Self {
            behavior: Mutex::new(behavior),
            spy: Arc::new(Mutex::new(BuilderSpyState::default())),
            is_busy: Mutex::new(false),
            output_dir: std::env::temp_dir().join("mock-ephemeral-builder"),
        }
    }

    /// Get a clone of the spy state for test assertions.
    pub fn spy(&self) -> Arc<Mutex<BuilderSpyState>> {
        Arc::clone(&self.spy)
    }

    /// Set the behavior for subsequent builds.
    pub fn set_behavior(&self, behavior: MockBuildBehavior) {
        *self.behavior.lock().unwrap() = behavior;
    }

    /// Set the busy state.
    pub fn set_busy(&self, busy: bool) {
        *self.is_busy.lock().unwrap() = busy;
    }

    /// Get the number of build calls.
    pub fn build_count(&self) -> usize {
        self.spy.lock().unwrap().build_calls.len()
    }

    /// Get the number of cancel calls.
    pub fn cancel_count(&self) -> usize {
        self.spy.lock().unwrap().cancel_calls.len()
    }

    /// Check if a specific build ID was requested.
    pub fn was_build_requested(&self, build_id: &str) -> bool {
        self.spy
            .lock()
            .unwrap()
            .build_calls
            .iter()
            .any(|r| r.build_id == build_id)
    }
}

impl Default for MockEphemeralBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EphemeralBuilder for MockEphemeralBuilder {
    async fn build(&self, request: BuildRequest) -> Result<BuildResult, BuildError> {
        // Record the call
        {
            let mut spy = self.spy.lock().unwrap();
            spy.build_calls.push(request.clone());
        }

        let behavior = self.behavior.lock().unwrap().clone();

        match behavior {
            MockBuildBehavior::Success {
                delay,
                artifact_size,
            } => {
                // Simulate build time
                tokio::time::sleep(delay).await;

                // Create a mock artifact file
                let artifact_path = self.output_dir.join(format!("{}.unik", request.build_id));
                if let Some(parent) = artifact_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&artifact_path, vec![0u8; artifact_size]);

                Ok(BuildResult {
                    unikernel_path: artifact_path,
                    build_duration: delay,
                    logs: format!("Mock build completed for {}", request.build_id),
                    cache_key: format!("mock-cache-{}", request.build_id),
                })
            }
            MockBuildBehavior::Timeout => Err(BuildError::Timeout {
                elapsed: request.limits.timeout + Duration::from_secs(10),
                limit: request.limits.timeout,
            }),
            MockBuildBehavior::OutOfMemory { requested_mib } => Err(BuildError::OutOfMemory {
                requested_mib,
                limit_mib: request.limits.memory_mib,
            }),
            MockBuildBehavior::DiskFull { used_mib } => Err(BuildError::DiskFull {
                used_mib,
                limit_mib: request.limits.disk_mib,
            }),
            MockBuildBehavior::DockerfileError(msg) => Err(BuildError::DockerfileInvalid(msg)),
            MockBuildBehavior::NetworkError => {
                Err(BuildError::NetworkSetupFailed("Mock network error".into()))
            }
            MockBuildBehavior::ArtifactExtractionError(msg) => {
                Err(BuildError::ArtifactExtractionFailed(msg))
            }
            MockBuildBehavior::Busy => Err(BuildError::BuilderBusy("Mock builder is busy".into())),
        }
    }

    fn is_busy(&self) -> bool {
        let mut spy = self.spy.lock().unwrap();
        spy.is_busy_calls += 1;
        *self.is_busy.lock().unwrap()
    }

    async fn cancel(&self, build_id: &str) -> Result<(), BuildError> {
        let mut spy = self.spy.lock().unwrap();
        spy.cancel_calls.push(build_id.to_string());
        Ok(())
    }
}

/// Configurable behaviors for MockNetworkIsolator.
#[derive(Debug, Clone, Default)]
pub enum MockNetworkBehavior {
    /// All operations succeed
    #[default]
    HappyPath,
    /// TAP creation fails
    TapCreationFails(String),
    /// Firewall setup fails
    FirewallFails(String),
    /// DNS resolution fails
    DnsResolutionFails(String),
    /// Teardown fails
    TeardownFails(String),
}

/// Spy state for tracking mock network isolator calls.
#[derive(Debug, Default)]
pub struct NetworkSpyState {
    /// VM IDs for which TAPs were created
    pub create_tap_calls: Vec<String>,
    /// (tap_name, allowlist) pairs for apply_allowlist calls
    pub apply_allowlist_calls: Vec<(String, Vec<EgressEntry>)>,
    /// TAP names for which teardown was called
    pub teardown_calls: Vec<String>,
    /// Currently active TAPs
    pub active_taps: HashMap<String, TapConfig>,
}

/// Mock implementation of NetworkIsolator for testing.
pub struct MockNetworkIsolator {
    behavior: Mutex<MockNetworkBehavior>,
    spy: Arc<Mutex<NetworkSpyState>>,
}

impl MockNetworkIsolator {
    /// Create a new mock network isolator with happy path behavior.
    pub fn new() -> Self {
        Self {
            behavior: Mutex::new(MockNetworkBehavior::default()),
            spy: Arc::new(Mutex::new(NetworkSpyState::default())),
        }
    }

    /// Create a new mock network isolator with specified behavior.
    pub fn with_behavior(behavior: MockNetworkBehavior) -> Self {
        Self {
            behavior: Mutex::new(behavior),
            spy: Arc::new(Mutex::new(NetworkSpyState::default())),
        }
    }

    /// Get a clone of the spy state for test assertions.
    pub fn spy(&self) -> Arc<Mutex<NetworkSpyState>> {
        Arc::clone(&self.spy)
    }

    /// Set the behavior for subsequent operations.
    pub fn set_behavior(&self, behavior: MockNetworkBehavior) {
        *self.behavior.lock().unwrap() = behavior;
    }

    /// Get the number of active TAP devices.
    pub fn active_tap_count(&self) -> usize {
        self.spy.lock().unwrap().active_taps.len()
    }

    /// Check if a TAP device is currently active.
    pub fn is_tap_active(&self, tap_name: &str) -> bool {
        self.spy.lock().unwrap().active_taps.contains_key(tap_name)
    }

    /// Get the number of create_tap calls.
    pub fn create_tap_count(&self) -> usize {
        self.spy.lock().unwrap().create_tap_calls.len()
    }

    /// Get the number of teardown calls.
    pub fn teardown_count(&self) -> usize {
        self.spy.lock().unwrap().teardown_calls.len()
    }
}

impl Default for MockNetworkIsolator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkIsolator for MockNetworkIsolator {
    async fn create_tap(&self, vm_id: &str) -> Result<TapConfig, NetworkError> {
        let behavior = self.behavior.lock().unwrap().clone();

        // Record the call
        {
            let mut spy = self.spy.lock().unwrap();
            spy.create_tap_calls.push(vm_id.to_string());
        }

        match behavior {
            MockNetworkBehavior::TapCreationFails(msg) => Err(NetworkError::TapCreationFailed(msg)),
            _ => {
                let config = TapConfig::for_vm(vm_id);
                let mut spy = self.spy.lock().unwrap();
                spy.active_taps
                    .insert(config.tap_name.clone(), config.clone());
                Ok(config)
            }
        }
    }

    async fn apply_allowlist(
        &self,
        tap_name: &str,
        allowlist: &[EgressEntry],
    ) -> Result<(), NetworkError> {
        let behavior = self.behavior.lock().unwrap().clone();

        // Record the call
        {
            let mut spy = self.spy.lock().unwrap();
            spy.apply_allowlist_calls
                .push((tap_name.to_string(), allowlist.to_vec()));
        }

        match behavior {
            MockNetworkBehavior::FirewallFails(msg) => Err(NetworkError::FirewallError(msg)),
            MockNetworkBehavior::DnsResolutionFails(msg) => {
                Err(NetworkError::DnsResolutionFailed(msg))
            }
            _ => Ok(()),
        }
    }

    async fn teardown(&self, tap_name: &str) -> Result<(), NetworkError> {
        let behavior = self.behavior.lock().unwrap().clone();

        // Record the call
        {
            let mut spy = self.spy.lock().unwrap();
            spy.teardown_calls.push(tap_name.to_string());
            spy.active_taps.remove(tap_name);
        }

        match behavior {
            MockNetworkBehavior::TeardownFails(msg) => Err(NetworkError::TeardownFailed(msg)),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_builder_success() {
        let builder = MockEphemeralBuilder::new();
        let request = BuildRequest::new("test-123", "FROM alpine");

        let result = builder.build(request).await.unwrap();
        assert!(result.unikernel_path.to_str().unwrap().contains("test-123"));
        assert_eq!(builder.build_count(), 1);
    }

    #[tokio::test]
    async fn mock_builder_timeout() {
        let builder = MockEphemeralBuilder::with_behavior(MockBuildBehavior::Timeout);
        let request = BuildRequest::new("test-123", "FROM alpine");

        let err = builder.build(request).await.unwrap_err();
        assert!(matches!(err, BuildError::Timeout { .. }));
    }

    #[tokio::test]
    async fn mock_builder_tracks_calls() {
        let builder = MockEphemeralBuilder::new();

        builder
            .build(BuildRequest::new("build-1", "FROM alpine"))
            .await
            .unwrap();
        builder
            .build(BuildRequest::new("build-2", "FROM ubuntu"))
            .await
            .unwrap();

        assert_eq!(builder.build_count(), 2);
        assert!(builder.was_build_requested("build-1"));
        assert!(builder.was_build_requested("build-2"));
        assert!(!builder.was_build_requested("build-3"));
    }

    #[tokio::test]
    async fn mock_network_isolator_happy_path() {
        let isolator = MockNetworkIsolator::new();

        let config = isolator.create_tap("vm-123").await.unwrap();
        assert!(config.tap_name.contains("vm-123"));
        assert!(isolator.is_tap_active(&config.tap_name));

        isolator
            .apply_allowlist(&config.tap_name, &[EgressEntry::https("pypi.org")])
            .await
            .unwrap();

        isolator.teardown(&config.tap_name).await.unwrap();
        assert!(!isolator.is_tap_active(&config.tap_name));
    }

    #[tokio::test]
    async fn mock_network_isolator_tap_creation_fails() {
        let isolator = MockNetworkIsolator::with_behavior(MockNetworkBehavior::TapCreationFails(
            "no permissions".into(),
        ));

        let err = isolator.create_tap("vm-123").await.unwrap_err();
        assert!(matches!(err, NetworkError::TapCreationFailed(_)));
    }

    #[tokio::test]
    async fn mock_network_isolator_tracks_allowlist() {
        let isolator = MockNetworkIsolator::new();

        let config = isolator.create_tap("vm-123").await.unwrap();
        isolator
            .apply_allowlist(
                &config.tap_name,
                &[
                    EgressEntry::https("pypi.org"),
                    EgressEntry::https("crates.io"),
                ],
            )
            .await
            .unwrap();

        let spy = isolator.spy();
        let spy_guard = spy.lock().unwrap();
        assert_eq!(spy_guard.apply_allowlist_calls.len(), 1);
        assert_eq!(spy_guard.apply_allowlist_calls[0].1.len(), 2);
    }
}
