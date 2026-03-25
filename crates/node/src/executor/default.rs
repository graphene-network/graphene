//! Default job executor implementation.
//!
//! Wires together all executor components for the full job execution pipeline:
//! 1. Decompress code/input assets
//! 2. Cache lookup or build the unikernel
//! 3. Prepare the execution drive with code, input, and environment
//! 4. Run the VM with resource limits and timeout enforcement
//! 5. Process output

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use tracing::{debug, info, instrument, warn};

use super::drive::ExecutionDriveBuilder;
use super::output::OutputProcessor;
use super::runner::{RunnerError, VmmRunner};
use super::types::{ExecutionError, ExecutionRequest, ExecutionResult};
use super::JobExecutor;
use crate::cache::BuildCache;
use crate::types::{AssetData, Compression};

/// Configuration for the default job executor.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Whether to clean up drives after execution.
    pub cleanup_drives: bool,
    /// Maximum concurrent jobs (0 = unlimited).
    pub max_concurrent_jobs: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            cleanup_drives: true,
            max_concurrent_jobs: 0,
        }
    }
}

/// Cancellation handle for a running job.
#[derive(Debug)]
struct JobHandle {
    cancelled: AtomicBool,
}

impl JobHandle {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Default implementation of [`JobExecutor`].
///
/// Wires together drive builder, VMM runner, output processor, and build cache.
pub struct DefaultJobExecutor<D, R, O, B>
where
    D: ExecutionDriveBuilder,
    R: VmmRunner,
    O: OutputProcessor,
    B: BuildCache,
{
    drive_builder: Arc<D>,
    runner: Arc<R>,
    output_processor: Arc<O>,
    cache: Arc<B>,
    config: ExecutorConfig,
    running_jobs: RwLock<HashMap<String, Arc<JobHandle>>>,
}

impl<D, R, O, B> DefaultJobExecutor<D, R, O, B>
where
    D: ExecutionDriveBuilder,
    R: VmmRunner,
    O: OutputProcessor,
    B: BuildCache,
{
    /// Create a new executor with the given components.
    pub fn new(
        drive_builder: Arc<D>,
        runner: Arc<R>,
        output_processor: Arc<O>,
        cache: Arc<B>,
    ) -> Self {
        Self {
            drive_builder,
            runner,
            output_processor,
            cache,
            config: ExecutorConfig::default(),
            running_jobs: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new executor with custom configuration.
    pub fn with_config(
        drive_builder: Arc<D>,
        runner: Arc<R>,
        output_processor: Arc<O>,
        cache: Arc<B>,
        config: ExecutorConfig,
    ) -> Self {
        Self {
            drive_builder,
            runner,
            output_processor,
            cache,
            config,
            running_jobs: RwLock::new(HashMap::new()),
        }
    }

    /// Fetch asset data, handling both inline and hash-ref modes.
    fn get_asset_data(
        &self,
        asset: &AssetData,
        job_id: &str,
        compression: Compression,
    ) -> Result<Vec<u8>, ExecutionError> {
        let raw_bytes = match asset {
            AssetData::Inline { data } => {
                debug!(job_id, size = data.len(), "Using inline asset data");
                data.clone()
            }
            AssetData::Hash { .. } => {
                // TODO(#200): Implement HTTP-based asset fetching for hash refs
                return Err(ExecutionError::asset_fetch(
                    "Hash-ref asset fetching not yet implemented",
                ));
            }
        };

        self.decompress(raw_bytes, compression)
    }

    /// Decompress data if compression is enabled.
    fn decompress(
        &self,
        data: Vec<u8>,
        compression: Compression,
    ) -> Result<Vec<u8>, ExecutionError> {
        match compression {
            Compression::None => Ok(data),
            Compression::Zstd => zstd::decode_all(data.as_slice()).map_err(|e| {
                ExecutionError::decompression(format!("Zstd decompression failed: {}", e))
            }),
        }
    }

    /// Compute a cache key hash for the code asset.
    fn compute_code_hash(&self, asset: &AssetData) -> [u8; 32] {
        match asset {
            AssetData::Inline { data } => *blake3::hash(data).as_bytes(),
            AssetData::Hash { hash, .. } => *hash,
        }
    }

    /// Look up or build the kernel for the request.
    async fn get_kernel(
        &self,
        request: &ExecutionRequest,
    ) -> Result<std::path::PathBuf, ExecutionError> {
        let kernel_spec = request.manifest.runtime.clone();
        let code_hash_bytes: [u8; 32] = self.compute_code_hash(&request.assets.code);

        // TODO(#42): Extract requirements.txt from code blob for proper L3 cache key
        let requirements: Vec<String> = vec![];

        match self
            .cache
            .lookup(&kernel_spec, &requirements, &code_hash_bytes)
            .await
        {
            Ok(Some(result)) => {
                info!(
                    kernel = kernel_spec,
                    cache_level = ?result.level,
                    "Cache hit"
                );
                return Ok(result.path);
            }
            Ok(None) => {
                debug!(
                    kernel = kernel_spec,
                    "Cache miss, checking for pre-built kernel"
                );
            }
            Err(e) => {
                warn!(kernel = kernel_spec, error = %e, "Cache lookup failed, checking for pre-built kernel");
            }
        }

        if let Some(kernel_path) = self.find_prebuilt_kernel(&kernel_spec) {
            info!(kernel = kernel_spec, path = ?kernel_path, "Found pre-built kernel");
            return Ok(kernel_path);
        }

        // TODO(#43): Implement actual unikernel build
        Err(ExecutionError::build(format!(
            "Unikernel build not yet implemented for runtime: {}. Cache miss.",
            kernel_spec
        )))
    }

    /// Find a pre-built kernel at known paths.
    fn find_prebuilt_kernel(&self, runtime_spec: &str) -> Option<std::path::PathBuf> {
        let kernel_name = runtime_spec.replace(':', "-");
        let filename = format!("{}_fc-x86_64", kernel_name);

        let search_paths = [
            std::env::var("GRAPHENE_KERNEL_CACHE")
                .ok()
                .map(std::path::PathBuf::from),
            dirs::home_dir().map(|h| h.join(".graphene/cache/kernels")),
            Some(std::path::PathBuf::from("/usr/share/graphene/kernels")),
        ];

        for path_opt in search_paths.into_iter().flatten() {
            let kernel_path = path_opt.join(&filename);
            if kernel_path.exists() {
                debug!(
                    runtime = runtime_spec,
                    path = ?kernel_path,
                    "Found pre-built kernel"
                );
                return Some(kernel_path);
            }
        }

        debug!(runtime = runtime_spec, "No pre-built kernel found");
        None
    }

    fn check_cancelled(&self, handle: &JobHandle) -> Result<(), ExecutionError> {
        if handle.is_cancelled() {
            Err(ExecutionError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn register_job(&self, job_id: &str) -> Arc<JobHandle> {
        let handle = Arc::new(JobHandle::new());
        self.running_jobs
            .write()
            .unwrap()
            .insert(job_id.to_string(), Arc::clone(&handle));
        handle
    }

    fn unregister_job(&self, job_id: &str) {
        self.running_jobs.write().unwrap().remove(job_id);
    }
}

#[async_trait]
impl<D, R, O, B> JobExecutor for DefaultJobExecutor<D, R, O, B>
where
    D: ExecutionDriveBuilder + 'static,
    R: VmmRunner + 'static,
    O: OutputProcessor + 'static,
    B: BuildCache + 'static,
{
    #[instrument(skip(self, request), fields(job_id = %request.job_id))]
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, ExecutionError> {
        let job_id = request.job_id.clone();
        let handle = self.register_job(&job_id);
        let result = self.execute_inner(&request, &handle).await;
        self.unregister_job(&job_id);
        result
    }

    async fn cancel(&self, job_id: &str) -> bool {
        let handle = self.running_jobs.read().unwrap().get(job_id).cloned();
        if let Some(handle) = handle {
            info!(job_id, "Cancelling job");
            handle.cancel();
            true
        } else {
            debug!(job_id, "Job not found for cancellation");
            false
        }
    }

    async fn is_running(&self, job_id: &str) -> bool {
        self.running_jobs.read().unwrap().contains_key(job_id)
    }
}

impl<D, R, O, B> DefaultJobExecutor<D, R, O, B>
where
    D: ExecutionDriveBuilder + 'static,
    R: VmmRunner + 'static,
    O: OutputProcessor + 'static,
    B: BuildCache + 'static,
{
    async fn execute_inner(
        &self,
        request: &ExecutionRequest,
        handle: &JobHandle,
    ) -> Result<ExecutionResult, ExecutionError> {
        let job_id = &request.job_id;
        info!(job_id, kernel = %request.manifest.runtime, "Starting job execution");

        // Phase 1: Get code asset
        self.check_cancelled(handle)?;
        let compression = request.assets.compression;
        let code = self.get_asset_data(&request.assets.code, job_id, compression)?;
        info!(job_id, code_size = code.len(), "Code asset ready");

        // Phase 2: Get input asset (if present)
        self.check_cancelled(handle)?;
        let input = if let Some(ref input_asset) = request.assets.input {
            Some(self.get_asset_data(input_asset, job_id, compression)?)
        } else {
            debug!(job_id, "No input asset specified");
            None
        };

        // Phase 3: Cache lookup or build kernel
        self.check_cancelled(handle)?;
        let kernel_path = self.get_kernel(request).await?;
        info!(job_id, kernel_path = %kernel_path.display(), "Kernel ready");

        // Phase 4: Prepare execution drive
        self.check_cancelled(handle)?;
        let drive_path = self
            .drive_builder
            .prepare(
                job_id,
                &code,
                input.as_deref(),
                &request.manifest.env,
                &request.manifest,
            )
            .await?;
        info!(job_id, drive_path = %drive_path.display(), "Execution drive prepared");

        // Phase 5: Run VM
        self.check_cancelled(handle)?;
        let boot_args = if request.manifest.runtime.starts_with("python") {
            "console=ttyS0 vfs.fstab=[ \"initrd0:/:extract:::\" ] -- /usr/bin/python3 /app/main.py"
                .to_string()
        } else if request.manifest.runtime.starts_with("node") {
            "console=ttyS0 vfs.fstab=[ \"initrd0:/:extract:::\" ] -- /usr/bin/node /app/index.js"
                .to_string()
        } else {
            "console=ttyS0 vfs.fstab=[ \"initrd0:/:extract:::\" ]".to_string()
        };

        let vmm_output = self
            .runner
            .run(&kernel_path, &drive_path, &request.manifest, &boot_args)
            .await
            .map_err(|e| match e {
                RunnerError::Timeout(d) => ExecutionError::timeout(d),
                RunnerError::ConfigurationFailed(msg) => {
                    ExecutionError::vmm(format!("Configuration failed: {}", msg))
                }
                RunnerError::BootSourceFailed(msg) => {
                    ExecutionError::vmm(format!("Boot source failed: {}", msg))
                }
                RunnerError::DriveAttachFailed(msg) => {
                    ExecutionError::vmm(format!("Drive attach failed: {}", msg))
                }
                RunnerError::StartFailed(msg) => {
                    ExecutionError::vmm(format!("Start failed: {}", msg))
                }
                RunnerError::Crashed(msg) => ExecutionError::vmm(format!("VM crashed: {}", msg)),
                RunnerError::OutputCaptureFailed(msg) => {
                    ExecutionError::vmm(format!("Output capture failed: {}", msg))
                }
                RunnerError::KernelNotFound(msg) => {
                    ExecutionError::vmm(format!("Kernel not found: {}", msg))
                }
                RunnerError::IoError(e) => ExecutionError::vmm(format!("I/O error: {}", e)),
            })?;

        info!(
            job_id,
            exit_code = vmm_output.exit_code,
            duration_ms = vmm_output.duration.as_millis() as u64,
            timed_out = vmm_output.timed_out,
            "VM execution completed"
        );

        // Phase 6: Process output
        let result = self
            .output_processor
            .process(
                &drive_path,
                vmm_output.stdout,
                vmm_output.stderr,
                vmm_output.exit_code,
                vmm_output.duration,
            )
            .await?;

        info!(
            job_id,
            exit_code = result.exit_code,
            "Output processed"
        );

        // Phase 7: Cleanup drive (if configured)
        if self.config.cleanup_drives {
            if let Err(e) = self.drive_builder.cleanup(&drive_path).await {
                warn!(job_id, error = %e, "Failed to cleanup drive");
            }
        }

        Ok(result)
    }
}

// ============================================================================
// Mock Implementation
// ============================================================================

pub mod mock {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[derive(Clone)]
    #[allow(clippy::type_complexity)]
    pub enum MockExecutorBehavior {
        Success { exit_code: i32, duration: Duration },
        Failure(String),
        Timeout,
        Cancelled,
        Custom(
            Arc<dyn Fn(&ExecutionRequest) -> Result<ExecutionResult, ExecutionError> + Send + Sync>,
        ),
    }

    impl Default for MockExecutorBehavior {
        fn default() -> Self {
            Self::Success {
                exit_code: 0,
                duration: Duration::from_millis(100),
            }
        }
    }

    impl std::fmt::Debug for MockExecutorBehavior {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Success { exit_code, duration } => f
                    .debug_struct("Success")
                    .field("exit_code", exit_code)
                    .field("duration", duration)
                    .finish(),
                Self::Failure(msg) => f.debug_struct("Failure").field("message", msg).finish(),
                Self::Timeout => write!(f, "Timeout"),
                Self::Cancelled => write!(f, "Cancelled"),
                Self::Custom(_) => write!(f, "Custom(...)"),
            }
        }
    }

    pub struct MockJobExecutor {
        behavior: std::sync::Mutex<MockExecutorBehavior>,
        call_count: AtomicUsize,
        running_jobs: RwLock<HashMap<String, Arc<JobHandle>>>,
    }

    impl std::fmt::Debug for MockJobExecutor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("MockJobExecutor")
                .field("behavior", &self.behavior)
                .field("call_count", &self.call_count)
                .finish()
        }
    }

    impl Default for MockJobExecutor {
        fn default() -> Self {
            Self::new(MockExecutorBehavior::default())
        }
    }

    impl MockJobExecutor {
        pub fn new(behavior: MockExecutorBehavior) -> Self {
            Self {
                behavior: std::sync::Mutex::new(behavior),
                call_count: AtomicUsize::new(0),
                running_jobs: RwLock::new(HashMap::new()),
            }
        }

        pub fn success() -> Self {
            Self::new(MockExecutorBehavior::default())
        }

        pub fn failing(error: impl Into<String>) -> Self {
            Self::new(MockExecutorBehavior::Failure(error.into()))
        }

        pub fn set_behavior(&self, behavior: MockExecutorBehavior) {
            *self.behavior.lock().unwrap() = behavior;
        }

        pub fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl JobExecutor for MockJobExecutor {
        async fn execute(
            &self,
            request: ExecutionRequest,
        ) -> Result<ExecutionResult, ExecutionError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);

            let handle = Arc::new(JobHandle::new());
            self.running_jobs
                .write()
                .unwrap()
                .insert(request.job_id.clone(), Arc::clone(&handle));

            tokio::time::sleep(Duration::from_millis(10)).await;

            if handle.is_cancelled() {
                self.running_jobs.write().unwrap().remove(&request.job_id);
                return Err(ExecutionError::Cancelled);
            }

            self.running_jobs.write().unwrap().remove(&request.job_id);

            let behavior = self.behavior.lock().unwrap().clone();
            match behavior {
                MockExecutorBehavior::Success { exit_code, duration } => {
                    Ok(ExecutionResult::new(
                        exit_code,
                        duration,
                        b"mock_result".to_vec(),
                        b"mock_stdout".to_vec(),
                        vec![],
                    ))
                }
                MockExecutorBehavior::Failure(msg) => Err(ExecutionError::vmm(msg)),
                MockExecutorBehavior::Timeout => {
                    Err(ExecutionError::timeout(Duration::from_secs(30)))
                }
                MockExecutorBehavior::Cancelled => Err(ExecutionError::Cancelled),
                MockExecutorBehavior::Custom(handler) => handler(&request),
            }
        }

        async fn cancel(&self, job_id: &str) -> bool {
            let handle = self.running_jobs.read().unwrap().get(job_id).cloned();
            if let Some(handle) = handle {
                handle.cancel();
                true
            } else {
                false
            }
        }

        async fn is_running(&self, job_id: &str) -> bool {
            self.running_jobs.read().unwrap().contains_key(job_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{MockExecutorBehavior, MockJobExecutor};
    use super::*;
    use crate::types::{JobAssets, JobManifest};
    use std::time::Duration;

    fn make_test_request(job_id: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            job_id,
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                runtime: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets::inline(b"print('hi')".to_vec(), None),
        )
    }

    #[tokio::test]
    async fn test_mock_executor_success() {
        let executor = MockJobExecutor::success();
        let request = make_test_request("job-1");

        let result = executor.execute(request).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.succeeded());
        assert_eq!(executor.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_executor_failure() {
        let executor = MockJobExecutor::failing("test error");
        let request = make_test_request("job-2");

        let result = executor.execute(request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExecutionError::VmmError(_)));
    }

    #[tokio::test]
    async fn test_mock_executor_timeout() {
        let executor = MockJobExecutor::new(MockExecutorBehavior::Timeout);
        let request = make_test_request("job-3");

        let result = executor.execute(request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExecutionError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_mock_executor_is_running() {
        let executor = Arc::new(MockJobExecutor::success());
        assert!(!executor.is_running("job-4").await);
    }

    #[tokio::test]
    async fn test_mock_executor_cancel() {
        let executor = MockJobExecutor::success();
        assert!(!executor.cancel("nonexistent").await);
    }

    #[tokio::test]
    async fn test_mock_executor_custom_behavior() {
        let executor = MockJobExecutor::new(MockExecutorBehavior::Custom(Arc::new(|req| {
            if req.job_id == "special" {
                Ok(ExecutionResult::new(
                    42,
                    Duration::from_millis(200),
                    b"special_result".to_vec(),
                    vec![],
                    vec![],
                ))
            } else {
                Err(ExecutionError::vmm("not special"))
            }
        })));

        let special_request = ExecutionRequest::new(
            "special",
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                runtime: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets::inline(b"print('hi')".to_vec(), None),
        );
        let result = executor.execute(special_request).await.unwrap();
        assert_eq!(result.exit_code, 42);

        let other_request = make_test_request("other");
        let result = executor.execute(other_request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_executor_call_count() {
        let executor = MockJobExecutor::success();

        for i in 0..5 {
            let request = make_test_request(&format!("job-{}", i));
            let _ = executor.execute(request).await;
        }

        assert_eq!(executor.call_count(), 5);
    }

    #[tokio::test]
    async fn test_executor_config_default() {
        let config = ExecutorConfig::default();
        assert!(config.cleanup_drives);
        assert_eq!(config.max_concurrent_jobs, 0);
    }

    #[tokio::test]
    async fn test_trait_object_safe() {
        let executor: Box<dyn JobExecutor> = Box::new(MockJobExecutor::success());
        let request = make_test_request("trait-object-test");
        let result = executor.execute(request).await;
        assert!(result.is_ok());
    }
}
