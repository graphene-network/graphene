//! Mock worker discovery implementation for testing.

use super::{DiscoveryError, JobRequirements, WorkerDiscovery, WorkerInfo};
use crate::p2p::messages::WorkerLoad;
use async_trait::async_trait;
use std::sync::{Arc, RwLock};

/// Configurable behaviors for the mock discovery service.
#[derive(Debug, Clone, Default)]
pub enum MockDiscoveryBehavior {
    /// Normal operation - all operations succeed.
    #[default]
    HappyPath,

    /// Start fails.
    StartFailure,

    /// Stop fails.
    StopFailure,

    /// Load update fails.
    LoadUpdateFailure,
}

/// Spy state for tracking operations in tests.
#[derive(Debug, Default)]
pub struct MockDiscoverySpyState {
    /// Whether start() was called.
    pub start_called: bool,

    /// Whether stop() was called.
    pub stop_called: bool,

    /// All find_workers() calls.
    pub find_workers_calls: Vec<JobRequirements>,

    /// All update_load() calls.
    pub load_updates: Vec<WorkerLoad>,
}

/// Mock implementation of [`WorkerDiscovery`] for testing.
pub struct MockWorkerDiscovery {
    /// Current behavior mode.
    behavior: Arc<RwLock<MockDiscoveryBehavior>>,

    /// Injected workers for testing.
    workers: Arc<RwLock<Vec<WorkerInfo>>>,

    /// Spy state for assertions.
    spy: Arc<RwLock<MockDiscoverySpyState>>,

    /// Whether the service is running.
    running: Arc<RwLock<bool>>,
}

impl MockWorkerDiscovery {
    /// Create a new mock discovery service with happy path behavior.
    pub fn new() -> Self {
        Self {
            behavior: Arc::new(RwLock::new(MockDiscoveryBehavior::HappyPath)),
            workers: Arc::new(RwLock::new(Vec::new())),
            spy: Arc::new(RwLock::new(MockDiscoverySpyState::default())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a mock with a specific behavior.
    pub fn with_behavior(behavior: MockDiscoveryBehavior) -> Self {
        let mock = Self::new();
        *mock.behavior.write().unwrap() = behavior;
        mock
    }

    /// Get access to the spy state for assertions.
    pub fn spy(&self) -> impl std::ops::Deref<Target = MockDiscoverySpyState> + '_ {
        self.spy.read().unwrap()
    }

    /// Set the mock behavior.
    pub fn set_behavior(&self, behavior: MockDiscoveryBehavior) {
        *self.behavior.write().unwrap() = behavior;
    }

    /// Inject a worker for testing.
    pub fn inject_worker(&self, worker: WorkerInfo) {
        self.workers.write().unwrap().push(worker);
    }

    /// Inject multiple workers for testing.
    pub fn inject_workers(&self, workers: Vec<WorkerInfo>) {
        self.workers.write().unwrap().extend(workers);
    }

    /// Clear all injected workers.
    pub fn clear_workers(&self) {
        self.workers.write().unwrap().clear();
    }

    /// Check if the service is running.
    pub fn is_running(&self) -> bool {
        *self.running.read().unwrap()
    }
}

impl Default for MockWorkerDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MockWorkerDiscovery {
    fn clone(&self) -> Self {
        Self {
            behavior: Arc::clone(&self.behavior),
            workers: Arc::clone(&self.workers),
            spy: Arc::clone(&self.spy),
            running: Arc::clone(&self.running),
        }
    }
}

#[async_trait]
impl WorkerDiscovery for MockWorkerDiscovery {
    async fn start(&self) -> Result<(), DiscoveryError> {
        self.spy.write().unwrap().start_called = true;

        if matches!(
            *self.behavior.read().unwrap(),
            MockDiscoveryBehavior::StartFailure
        ) {
            return Err(DiscoveryError::NotRunning);
        }

        let mut running = self.running.write().unwrap();
        if *running {
            return Err(DiscoveryError::AlreadyRunning);
        }
        *running = true;

        Ok(())
    }

    async fn stop(&self) -> Result<(), DiscoveryError> {
        self.spy.write().unwrap().stop_called = true;

        if matches!(
            *self.behavior.read().unwrap(),
            MockDiscoveryBehavior::StopFailure
        ) {
            return Err(DiscoveryError::NotRunning);
        }

        let mut running = self.running.write().unwrap();
        if !*running {
            return Err(DiscoveryError::NotRunning);
        }
        *running = false;

        Ok(())
    }

    async fn find_workers(&self, requirements: &JobRequirements) -> Vec<WorkerInfo> {
        self.spy
            .write()
            .unwrap()
            .find_workers_calls
            .push(requirements.clone());

        self.workers
            .read()
            .unwrap()
            .iter()
            .filter(|w| w.meets_requirements(requirements))
            .cloned()
            .collect()
    }

    async fn list_workers(&self) -> Vec<WorkerInfo> {
        self.workers.read().unwrap().clone()
    }

    async fn update_load(&self, load: WorkerLoad) -> Result<(), DiscoveryError> {
        if matches!(
            *self.behavior.read().unwrap(),
            MockDiscoveryBehavior::LoadUpdateFailure
        ) {
            return Err(DiscoveryError::NotRunning);
        }

        if !*self.running.read().unwrap() {
            return Err(DiscoveryError::NotRunning);
        }

        self.spy.write().unwrap().load_updates.push(load);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::WorkerStatus;
    use crate::p2p::messages::{WorkerCapabilities, WorkerPricing};
    use iroh::SecretKey;
    use rand::RngCore;
    use std::time::Instant;

    fn make_test_worker(status: WorkerStatus, slots: u8) -> WorkerInfo {
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let key = SecretKey::from_bytes(&key_bytes);
        WorkerInfo {
            node_id: key.public(),
            addr: None,
            version: "0.1.0".to_string(),
            capabilities: WorkerCapabilities {
                max_vcpu: 8,
                max_memory_mb: 16384,
                kernels: vec!["node-20-unikraft".to_string()],
            },
            pricing: WorkerPricing::default(),
            load: WorkerLoad {
                available_slots: slots,
                queue_depth: 0,
            },
            status,
            last_seen: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_mock_start_stop() {
        let mock = MockWorkerDiscovery::new();

        assert!(!mock.is_running());

        mock.start().await.unwrap();
        assert!(mock.is_running());
        assert!(mock.spy().start_called);

        mock.stop().await.unwrap();
        assert!(!mock.is_running());
        assert!(mock.spy().stop_called);
    }

    #[tokio::test]
    async fn test_mock_double_start() {
        let mock = MockWorkerDiscovery::new();

        mock.start().await.unwrap();
        let result = mock.start().await;
        assert!(matches!(result, Err(DiscoveryError::AlreadyRunning)));
    }

    #[tokio::test]
    async fn test_mock_stop_without_start() {
        let mock = MockWorkerDiscovery::new();

        let result = mock.stop().await;
        assert!(matches!(result, Err(DiscoveryError::NotRunning)));
    }

    #[tokio::test]
    async fn test_mock_inject_and_find_workers() {
        let mock = MockWorkerDiscovery::new();
        mock.start().await.unwrap();

        let worker = make_test_worker(WorkerStatus::Online, 4);
        mock.inject_worker(worker);

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
        };

        let found = mock.find_workers(&requirements).await;
        assert_eq!(found.len(), 1);
        assert_eq!(mock.spy().find_workers_calls.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_find_excludes_non_matching() {
        let mock = MockWorkerDiscovery::new();
        mock.start().await.unwrap();

        // Inject an online worker
        mock.inject_worker(make_test_worker(WorkerStatus::Online, 4));
        // Inject an offline worker
        mock.inject_worker(make_test_worker(WorkerStatus::Offline, 4));

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
        };

        let found = mock.find_workers(&requirements).await;
        assert_eq!(found.len(), 1); // Only the online one
    }

    #[tokio::test]
    async fn test_mock_list_workers() {
        let mock = MockWorkerDiscovery::new();

        mock.inject_worker(make_test_worker(WorkerStatus::Online, 4));
        mock.inject_worker(make_test_worker(WorkerStatus::Offline, 2));

        let all = mock.list_workers().await;
        assert_eq!(all.len(), 2); // Includes offline
    }

    #[tokio::test]
    async fn test_mock_update_load() {
        let mock = MockWorkerDiscovery::new();
        mock.start().await.unwrap();

        let load = WorkerLoad {
            available_slots: 3,
            queue_depth: 1,
        };

        mock.update_load(load).await.unwrap();
        assert_eq!(mock.spy().load_updates.len(), 1);
        assert_eq!(mock.spy().load_updates[0].available_slots, 3);
    }

    #[tokio::test]
    async fn test_mock_update_load_requires_running() {
        let mock = MockWorkerDiscovery::new();

        let load = WorkerLoad::default();
        let result = mock.update_load(load).await;
        assert!(matches!(result, Err(DiscoveryError::NotRunning)));
    }

    #[tokio::test]
    async fn test_mock_behavior_start_failure() {
        let mock = MockWorkerDiscovery::with_behavior(MockDiscoveryBehavior::StartFailure);

        let result = mock.start().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_clear_workers() {
        let mock = MockWorkerDiscovery::new();
        mock.inject_worker(make_test_worker(WorkerStatus::Online, 4));

        assert_eq!(mock.list_workers().await.len(), 1);

        mock.clear_workers();
        assert_eq!(mock.list_workers().await.len(), 0);
    }
}
