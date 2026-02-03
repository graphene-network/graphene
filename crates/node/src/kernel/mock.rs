use super::{KernelError, KernelMetadata, KernelRegistry, KernelSpec};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Configurable behavior for mock kernel operations
#[derive(Debug, Clone, Default)]
pub enum MockBehavior {
    /// All operations succeed
    #[default]
    HappyPath,
    /// Downloads fail with network error
    DownloadFailure,
    /// Hash verification fails
    CorruptedKernel,
    /// Kernel not found in registry
    KernelNotFound,
    /// Simulate slow download (for timeout testing)
    SlowDownload { delay_ms: u64 },
}

/// Spy state for observing mock interactions
#[derive(Default, Debug)]
pub struct KernelSpyState {
    pub resolve_calls: usize,
    pub get_calls: usize,
    pub ensure_calls: usize,
    pub hits: usize,
    pub misses: usize,
    pub downloads: usize,
}

/// Mock implementation of KernelRegistry for testing
#[derive(Clone)]
pub struct MockKernelRegistry {
    behavior: MockBehavior,
    /// Pre-populated kernels (simulates cache)
    cached: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// Available kernels in the "registry"
    available: Arc<Mutex<Vec<KernelSpec>>>,
    /// Metadata for available kernels
    metadata: Arc<Mutex<HashMap<String, KernelMetadata>>>,
    /// Spy state for assertions
    pub spy: Arc<Mutex<KernelSpyState>>,
}

impl Default for MockKernelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MockKernelRegistry {
    pub fn new() -> Self {
        Self::with_behavior(MockBehavior::HappyPath)
    }

    pub fn with_behavior(behavior: MockBehavior) -> Self {
        let mut registry = Self {
            behavior,
            cached: Arc::new(Mutex::new(HashMap::new())),
            available: Arc::new(Mutex::new(Vec::new())),
            metadata: Arc::new(Mutex::new(HashMap::new())),
            spy: Arc::new(Mutex::new(KernelSpyState::default())),
        };

        // Pre-populate with default available kernels
        registry.add_default_kernels();
        registry
    }

    fn add_default_kernels(&mut self) {
        use super::types::Runtime;

        let defaults = vec![
            (Runtime::Python, "3.11"),
            (Runtime::Python, "3.12"),
            (Runtime::Node, "20"),
            (Runtime::Node, "22"),
            (Runtime::Bun, "1.x"),
            (Runtime::Deno, "2.x"),
        ];

        for (runtime, version) in defaults {
            let spec = KernelSpec::new(runtime, version);
            self.add_available(spec.clone());
            self.add_metadata(spec, default_metadata(runtime, version));
        }
    }

    /// Add a kernel to the available list
    pub fn add_available(&mut self, spec: KernelSpec) {
        self.available.lock().unwrap().push(spec);
    }

    /// Add metadata for a kernel
    pub fn add_metadata(&mut self, spec: KernelSpec, metadata: KernelMetadata) {
        self.metadata
            .lock()
            .unwrap()
            .insert(spec.canonical_name(), metadata);
    }

    /// Pre-cache a kernel (simulates already downloaded)
    pub fn pre_cache(&mut self, spec: &KernelSpec, path: PathBuf) {
        self.cached
            .lock()
            .unwrap()
            .insert(spec.canonical_name(), path);
    }

    /// Get spy state for assertions
    pub fn get_spy_state(&self) -> KernelSpyState {
        let spy = self.spy.lock().unwrap();
        KernelSpyState {
            resolve_calls: spy.resolve_calls,
            get_calls: spy.get_calls,
            ensure_calls: spy.ensure_calls,
            hits: spy.hits,
            misses: spy.misses,
            downloads: spy.downloads,
        }
    }
}

fn default_metadata(runtime: super::types::Runtime, version: &str) -> KernelMetadata {
    KernelMetadata {
        spec: KernelSpec::new(runtime, version),
        binary_hash: format!("mock_hash_{}_{}", runtime, version),
        binary_size_bytes: 50 * 1024 * 1024, // 50MB
        min_memory_mib: 128,
        recommended_memory_mib: 256,
        default_boot_args: "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules".to_string(),
        unikraft_version: "0.17.0".to_string(),
        built_at: Some("2025-01-01T00:00:00Z".to_string()),
    }
}

#[async_trait]
impl KernelRegistry for MockKernelRegistry {
    fn resolve(&self, name: &str) -> Result<KernelSpec, KernelError> {
        self.spy.lock().unwrap().resolve_calls += 1;

        if matches!(self.behavior, MockBehavior::KernelNotFound) {
            return Err(KernelError::NotFound(name.to_string()));
        }

        KernelSpec::parse(name).map_err(KernelError::InvalidSpec)
    }

    async fn get(&self, spec: &KernelSpec) -> Result<Option<PathBuf>, KernelError> {
        let mut spy = self.spy.lock().unwrap();
        spy.get_calls += 1;

        let cached = self.cached.lock().unwrap();
        if let Some(path) = cached.get(&spec.canonical_name()) {
            spy.hits += 1;
            Ok(Some(path.clone()))
        } else {
            spy.misses += 1;
            Ok(None)
        }
    }

    async fn ensure(&self, spec: &KernelSpec) -> Result<PathBuf, KernelError> {
        self.spy.lock().unwrap().ensure_calls += 1;

        // Check cache first
        if let Some(path) = self.get(spec).await? {
            return Ok(path);
        }

        // Simulate behavior
        match &self.behavior {
            MockBehavior::HappyPath => {
                self.spy.lock().unwrap().downloads += 1;
                let path = PathBuf::from(format!("/tmp/mock_kernels/{}", spec.canonical_name()));
                self.cached
                    .lock()
                    .unwrap()
                    .insert(spec.canonical_name(), path.clone());
                Ok(path)
            }
            MockBehavior::DownloadFailure => Err(KernelError::NetworkError(
                "mock download failure".to_string(),
            )),
            MockBehavior::CorruptedKernel => Err(KernelError::HashMismatch {
                expected: "expected_hash".to_string(),
                actual: "corrupted_hash".to_string(),
            }),
            MockBehavior::KernelNotFound => Err(KernelError::NotFound(spec.canonical_name())),
            MockBehavior::SlowDownload { delay_ms } => {
                tokio::time::sleep(tokio::time::Duration::from_millis(*delay_ms)).await;
                self.spy.lock().unwrap().downloads += 1;
                let path = PathBuf::from(format!("/tmp/mock_kernels/{}", spec.canonical_name()));
                self.cached
                    .lock()
                    .unwrap()
                    .insert(spec.canonical_name(), path.clone());
                Ok(path)
            }
        }
    }

    fn list_available(&self) -> Vec<KernelSpec> {
        self.available.lock().unwrap().clone()
    }

    fn get_metadata(&self, spec: &KernelSpec) -> Result<KernelMetadata, KernelError> {
        self.metadata
            .lock()
            .unwrap()
            .get(&spec.canonical_name())
            .cloned()
            .ok_or_else(|| KernelError::NotFound(spec.canonical_name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_happy_path() {
        let registry = MockKernelRegistry::new();
        let spec = KernelSpec::parse("python-3.11").unwrap();

        // First call should miss cache
        let result = registry.get(&spec).await.unwrap();
        assert!(result.is_none());

        // Ensure should download
        let path = registry.ensure(&spec).await.unwrap();
        assert!(path.to_string_lossy().contains("python-3.11"));

        // Second call should hit cache
        let result = registry.get(&spec).await.unwrap();
        assert!(result.is_some());

        let spy = registry.get_spy_state();
        assert_eq!(spy.misses, 2); // First get + ensure's internal get
        assert_eq!(spy.hits, 1); // Second get after download
        assert_eq!(spy.downloads, 1);
    }

    #[tokio::test]
    async fn test_mock_download_failure() {
        let registry = MockKernelRegistry::with_behavior(MockBehavior::DownloadFailure);
        let spec = KernelSpec::parse("python-3.11").unwrap();

        let result = registry.ensure(&spec).await;
        assert!(matches!(result, Err(KernelError::NetworkError(_))));
    }

    #[tokio::test]
    async fn test_mock_corrupted_kernel() {
        let registry = MockKernelRegistry::with_behavior(MockBehavior::CorruptedKernel);
        let spec = KernelSpec::parse("python-3.11").unwrap();

        let result = registry.ensure(&spec).await;
        assert!(matches!(result, Err(KernelError::HashMismatch { .. })));
    }

    #[tokio::test]
    async fn test_list_available() {
        let registry = MockKernelRegistry::new();
        let available = registry.list_available();

        assert!(available.len() >= 6); // At least the defaults
        assert!(available
            .iter()
            .any(|s| s.runtime == super::super::Runtime::Python));
        assert!(available
            .iter()
            .any(|s| s.runtime == super::super::Runtime::Node));
    }

    #[tokio::test]
    async fn test_pre_cached_kernel() {
        let mut registry = MockKernelRegistry::new();
        let spec = KernelSpec::parse("python-3.11").unwrap();
        let cached_path = PathBuf::from("/cached/python-3.11.unik");

        registry.pre_cache(&spec, cached_path.clone());

        let result = registry.get(&spec).await.unwrap();
        assert_eq!(result, Some(cached_path));

        let spy = registry.get_spy_state();
        assert_eq!(spy.hits, 1);
        assert_eq!(spy.downloads, 0);
    }
}
