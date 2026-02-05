//! Default job executor implementation.
//!
//! This module provides [`DefaultJobExecutor`], which wires together all executor
//! components to implement the full job execution pipeline:
//!
//! 1. Fetch and decrypt code/input blobs from P2P network
//! 2. Cache lookup or build the unikernel
//! 3. Prepare the execution drive with code, input, and environment
//! 4. Run the VM with resource limits and timeout enforcement
//! 5. Process and encrypt the output
//!
//! # Example
//!
//! ```ignore
//! use monad_node::executor::{DefaultJobExecutor, ExecutionRequest, JobExecutor};
//!
//! // Create executor with all dependencies injected
//! let executor = DefaultJobExecutor::new(
//!     drive_builder,
//!     runner,
//!     output_processor,
//!     crypto,
//!     network,
//!     cache,
//! );
//!
//! // Execute a job
//! let result = executor.execute(request).await?;
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use iroh_blobs::Hash;
use tracing::{debug, info, instrument, warn};

use super::drive::ExecutionDriveBuilder;
use super::output::OutputProcessor;
use super::runner::{RunnerError, VmmRunner};
use super::types::{ExecutionError, ExecutionRequest, ExecutionResult};
use super::JobExecutor;
use crate::cache::BuildCache;
use crate::crypto::{ChannelKeys, CryptoProvider, EncryptedBlob, EncryptionDirection};
use crate::p2p::protocol::types::{AssetData, Compression};
use crate::p2p::P2PNetwork;

/// Default boot arguments for unikernels.
const DEFAULT_BOOT_ARGS: &str = "console=ttyS0 quiet";

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
    /// Flag indicating whether cancellation has been requested.
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
/// This executor wires together all components needed for job execution:
/// - Drive builder for creating ext4 images
/// - VMM runner for executing unikernels in Firecracker
/// - Output processor for encrypting results
/// - Crypto provider for decrypting inputs
/// - P2P network for fetching blobs
/// - Build cache for caching unikernel builds
///
/// # Thread Safety
///
/// The executor is fully thread-safe and can handle concurrent job executions.
/// Each job runs in isolation in its own MicroVM.
///
/// # Cancellation
///
/// Jobs can be cancelled via the [`cancel`] method. Cancellation is cooperative:
/// the job will be cancelled at the next cancellation point (typically before
/// each major phase).
pub struct DefaultJobExecutor<D, R, O, C, N, B>
where
    D: ExecutionDriveBuilder,
    R: VmmRunner,
    O: OutputProcessor,
    C: CryptoProvider,
    N: P2PNetwork,
    B: BuildCache,
{
    drive_builder: Arc<D>,
    runner: Arc<R>,
    output_processor: Arc<O>,
    crypto: Arc<C>,
    network: Arc<N>,
    cache: Arc<B>,
    config: ExecutorConfig,
    /// Tracks running jobs with their cancellation handles.
    running_jobs: RwLock<HashMap<String, Arc<JobHandle>>>,
    /// Worker's Ed25519 secret key for deriving channel keys.
    worker_secret: [u8; 32],
}

impl<D, R, O, C, N, B> DefaultJobExecutor<D, R, O, C, N, B>
where
    D: ExecutionDriveBuilder,
    R: VmmRunner,
    O: OutputProcessor,
    C: CryptoProvider,
    N: P2PNetwork,
    B: BuildCache,
{
    /// Create a new executor with the given components.
    pub fn new(
        drive_builder: Arc<D>,
        runner: Arc<R>,
        output_processor: Arc<O>,
        crypto: Arc<C>,
        network: Arc<N>,
        cache: Arc<B>,
        worker_secret: [u8; 32],
    ) -> Self {
        Self {
            drive_builder,
            runner,
            output_processor,
            crypto,
            network,
            cache,
            config: ExecutorConfig::default(),
            running_jobs: RwLock::new(HashMap::new()),
            worker_secret,
        }
    }

    /// Create a new executor with custom configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        drive_builder: Arc<D>,
        runner: Arc<R>,
        output_processor: Arc<O>,
        crypto: Arc<C>,
        network: Arc<N>,
        cache: Arc<B>,
        worker_secret: [u8; 32],
        config: ExecutorConfig,
    ) -> Self {
        Self {
            drive_builder,
            runner,
            output_processor,
            crypto,
            network,
            cache,
            config,
            running_jobs: RwLock::new(HashMap::new()),
            worker_secret,
        }
    }

    /// Derive channel keys for the given request.
    fn derive_channel_keys(
        &self,
        request: &ExecutionRequest,
    ) -> Result<ChannelKeys, ExecutionError> {
        self.crypto
            .derive_channel_keys(
                &self.worker_secret,
                &request.payer_pubkey,
                &request.channel_pda,
            )
            .map_err(|e| {
                ExecutionError::decryption(format!("Failed to derive channel keys: {}", e))
            })
    }

    /// Fetch a blob from the P2P network.
    ///
    /// If `client_node_id` is provided, attempts to download directly from the client.
    async fn fetch_blob(
        &self,
        hash: Hash,
        client_node_id: Option<&[u8; 32]>,
    ) -> Result<Vec<u8>, ExecutionError> {
        // Convert client node ID to EndpointAddr if provided
        let from = client_node_id.and_then(|id| {
            iroh::PublicKey::try_from(id.as_slice())
                .ok()
                .map(iroh::EndpointAddr::new)
        });

        self.network.download_blob(hash, from).await.map_err(|e| {
            ExecutionError::asset_fetch(format!("Failed to fetch blob {}: {}", hash, e))
        })
    }

    /// Fetch and decrypt an asset, handling both inline and blob modes.
    ///
    /// For inline assets, the data is already present and just needs decryption.
    /// For blob assets, fetches from the P2P network first.
    async fn fetch_asset(
        &self,
        asset: &AssetData,
        channel_keys: &ChannelKeys,
        job_id: &str,
        client_node_id: Option<&[u8; 32]>,
        compression: Compression,
    ) -> Result<Vec<u8>, ExecutionError> {
        let encrypted_bytes = match asset {
            AssetData::Inline { data } => {
                debug!(job_id, size = data.len(), "Using inline asset data");
                data.clone()
            }
            AssetData::Blob { hash, .. } => {
                debug!(job_id, blob = %hash, "Fetching blob asset");
                self.fetch_blob(*hash, client_node_id).await?
            }
        };

        // Parse encrypted blob format
        let encrypted = EncryptedBlob::from_bytes(&encrypted_bytes).map_err(|e| {
            ExecutionError::decryption(format!("Invalid encrypted blob format: {}", e))
        })?;

        // Decrypt
        let decrypted = self
            .crypto
            .decrypt_job_blob(&encrypted, channel_keys, job_id, EncryptionDirection::Input)
            .map_err(|e| ExecutionError::decryption(format!("Decryption failed: {}", e)))?;

        // Decompress if needed
        let result = self.decompress(decrypted, compression)?;

        debug!(
            job_id,
            decrypted_size = result.len(),
            compression = ?compression,
            "Asset fetched and decrypted successfully"
        );

        Ok(result)
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
                ExecutionError::decryption(format!("Zstd decompression failed: {}", e))
            }),
        }
    }

    /// Compute a cache key hash for the code asset.
    ///
    /// For inline assets, computes a hash of the data.
    /// For blob assets, uses the existing blob hash.
    fn compute_code_hash(&self, asset: &AssetData) -> [u8; 32] {
        match asset {
            AssetData::Inline { data } => *blake3::hash(data).as_bytes(),
            AssetData::Blob { hash, .. } => *hash.as_bytes(),
        }
    }

    /// Look up or build the kernel for the request.
    async fn get_kernel(
        &self,
        request: &ExecutionRequest,
    ) -> Result<std::path::PathBuf, ExecutionError> {
        let kernel_spec = request.manifest.kernel.clone();
        let code_hash_bytes: [u8; 32] = self.compute_code_hash(&request.assets.code);

        // For now, we don't parse requirements from the code blob
        // TODO(#42): Extract requirements.txt from code blob for proper L3 cache key
        let requirements: Vec<String> = vec![];

        // Check cache
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

        // Check for pre-built kernel at known paths
        // This supports CI/testing scenarios where kernels are pre-built but not in the cache
        if let Some(kernel_path) = self.find_prebuilt_kernel(&kernel_spec) {
            info!(kernel = kernel_spec, path = ?kernel_path, "Found pre-built kernel");
            return Ok(kernel_path);
        }

        // TODO(#43): Implement actual unikernel build
        // For now, return an error indicating build is not implemented
        Err(ExecutionError::build(format!(
            "Unikernel build not yet implemented for kernel: {}. Cache miss.",
            kernel_spec
        )))
    }

    /// Find a pre-built kernel at known paths.
    ///
    /// Checks for kernels in:
    /// 1. GRAPHENE_KERNEL_CACHE environment variable
    /// 2. $HOME/.graphene/cache/kernels
    /// 3. /usr/share/graphene/kernels
    fn find_prebuilt_kernel(&self, kernel_spec: &str) -> Option<std::path::PathBuf> {
        // Convert kernel spec like "python:3.12" to filename like "python-3.12_fc-x86_64"
        let kernel_name = kernel_spec.replace(':', "-");
        let filename = format!("{}_fc-x86_64", kernel_name);

        // Check paths in priority order
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
                    kernel = kernel_spec,
                    path = ?kernel_path,
                    "Found pre-built kernel"
                );
                return Some(kernel_path);
            }
        }

        debug!(kernel = kernel_spec, "No pre-built kernel found");
        None
    }

    /// Check cancellation and return error if cancelled.
    fn check_cancelled(&self, handle: &JobHandle) -> Result<(), ExecutionError> {
        if handle.is_cancelled() {
            Err(ExecutionError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Register a job as running.
    fn register_job(&self, job_id: &str) -> Arc<JobHandle> {
        let handle = Arc::new(JobHandle::new());
        self.running_jobs
            .write()
            .unwrap()
            .insert(job_id.to_string(), Arc::clone(&handle));
        handle
    }

    /// Unregister a job.
    fn unregister_job(&self, job_id: &str) {
        self.running_jobs.write().unwrap().remove(job_id);
    }
}

#[async_trait]
impl<D, R, O, C, N, B> JobExecutor for DefaultJobExecutor<D, R, O, C, N, B>
where
    D: ExecutionDriveBuilder + 'static,
    R: VmmRunner + 'static,
    O: OutputProcessor + 'static,
    C: CryptoProvider + 'static,
    N: P2PNetwork + 'static,
    B: BuildCache + 'static,
{
    #[instrument(skip(self, request), fields(job_id = %request.job_id))]
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, ExecutionError> {
        let job_id = request.job_id.clone();

        // Register the job as running
        let handle = self.register_job(&job_id);

        // Execute and ensure cleanup
        let result = self.execute_inner(&request, &handle).await;

        // Always unregister the job
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

impl<D, R, O, C, N, B> DefaultJobExecutor<D, R, O, C, N, B>
where
    D: ExecutionDriveBuilder + 'static,
    R: VmmRunner + 'static,
    O: OutputProcessor + 'static,
    C: CryptoProvider + 'static,
    N: P2PNetwork + 'static,
    B: BuildCache + 'static,
{
    /// Inner execution logic, separated for cleaner error handling.
    async fn execute_inner(
        &self,
        request: &ExecutionRequest,
        handle: &JobHandle,
    ) -> Result<ExecutionResult, ExecutionError> {
        let job_id = &request.job_id;
        info!(job_id, kernel = %request.manifest.kernel, "Starting job execution");

        // Phase 1: Derive channel keys
        self.check_cancelled(handle)?;
        let channel_keys = self.derive_channel_keys(request)?;
        debug!(job_id, "Channel keys derived");

        // Phase 2: Fetch and decrypt code asset
        self.check_cancelled(handle)?;
        let client_node_id = request.client_node_id.as_ref();
        let compression = request.assets.compression;
        let code = self
            .fetch_asset(
                &request.assets.code,
                &channel_keys,
                job_id,
                client_node_id,
                compression,
            )
            .await?;
        info!(
            job_id,
            code_size = code.len(),
            "Code asset fetched and decrypted"
        );

        // Phase 3: Fetch and decrypt input asset (if present)
        self.check_cancelled(handle)?;
        let input = if let Some(ref input_asset) = request.assets.input {
            Some(
                self.fetch_asset(
                    input_asset,
                    &channel_keys,
                    job_id,
                    client_node_id,
                    compression,
                )
                .await?,
            )
        } else {
            debug!(job_id, "No input asset specified");
            None
        };

        // Phase 4: Cache lookup or build kernel
        self.check_cancelled(handle)?;
        let kernel_path = self.get_kernel(request).await?;
        info!(job_id, kernel_path = %kernel_path.display(), "Kernel ready");

        // Phase 5: Prepare execution drive
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

        // Phase 6: Run VM
        self.check_cancelled(handle)?;
        let vmm_output = self
            .runner
            .run(
                &kernel_path,
                &drive_path,
                &request.manifest,
                DEFAULT_BOOT_ARGS,
            )
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

        // Phase 7: Process and encrypt output
        let result = self
            .output_processor
            .process(
                &drive_path,
                vmm_output.stdout,
                vmm_output.stderr,
                vmm_output.exit_code,
                vmm_output.duration,
                request,
                &channel_keys,
            )
            .await?;

        info!(
            job_id,
            exit_code = result.exit_code,
            result_hash = %result.result_hash,
            "Output processed"
        );

        // Phase 8: Cleanup drive (if configured)
        if self.config.cleanup_drives {
            if let Err(e) = self.drive_builder.cleanup(&drive_path).await {
                warn!(job_id, error = %e, "Failed to cleanup drive");
                // Don't fail the job for cleanup errors
            }
        }

        Ok(result)
    }
}

// ============================================================================
// Mock Implementation
// ============================================================================

/// Mock implementation of [`JobExecutor`] for testing.
///
/// Configurable behavior allows testing various scenarios without
/// requiring real infrastructure.
pub mod mock {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Behavior configuration for mock executor.
    #[derive(Clone)]
    #[allow(clippy::type_complexity)]
    pub enum MockExecutorBehavior {
        /// Always succeed with the given result.
        Success { exit_code: i32, duration: Duration },
        /// Always fail with the given error.
        Failure(String),
        /// Simulate timeout.
        Timeout,
        /// Cancelled.
        Cancelled,
        /// Custom handler.
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
                Self::Success {
                    exit_code,
                    duration,
                } => f
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

    /// Mock job executor for testing.
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
        /// Create a new mock executor with the given behavior.
        pub fn new(behavior: MockExecutorBehavior) -> Self {
            Self {
                behavior: std::sync::Mutex::new(behavior),
                call_count: AtomicUsize::new(0),
                running_jobs: RwLock::new(HashMap::new()),
            }
        }

        /// Create a mock that always succeeds.
        pub fn success() -> Self {
            Self::new(MockExecutorBehavior::default())
        }

        /// Create a mock that always fails.
        pub fn failing(error: impl Into<String>) -> Self {
            Self::new(MockExecutorBehavior::Failure(error.into()))
        }

        /// Set the behavior for subsequent calls.
        pub fn set_behavior(&self, behavior: MockExecutorBehavior) {
            *self.behavior.lock().unwrap() = behavior;
        }

        /// Get the number of execute calls.
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

            // Simulate some work
            tokio::time::sleep(Duration::from_millis(10)).await;

            // Check for cancellation
            if handle.is_cancelled() {
                self.running_jobs.write().unwrap().remove(&request.job_id);
                return Err(ExecutionError::Cancelled);
            }

            self.running_jobs.write().unwrap().remove(&request.job_id);

            let behavior = self.behavior.lock().unwrap().clone();
            match behavior {
                MockExecutorBehavior::Success {
                    exit_code,
                    duration,
                } => Ok(ExecutionResult::new(
                    exit_code,
                    duration,
                    b"mock_result".to_vec(),
                    b"mock_stdout".to_vec(),
                    vec![],
                    Hash::new(b"mock_result"),
                )),
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
    use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
    use crate::p2p::protocol::types::JobAssets;
    use std::time::Duration;

    fn make_test_request(job_id: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            job_id,
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets::blobs(Hash::from_bytes([1u8; 32]), None),
            [0u8; 32],
            [0u8; 32],
            [0u8; 32],
            ResultDeliveryMode::Sync,
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

        // Job not running yet
        assert!(!executor.is_running("job-4").await);
    }

    #[tokio::test]
    async fn test_mock_executor_cancel() {
        let executor = MockJobExecutor::success();

        // Can't cancel non-existent job
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
                    Hash::new(b"special_result"),
                ))
            } else {
                Err(ExecutionError::vmm("not special"))
            }
        })));

        // Special job succeeds
        let special_request = ExecutionRequest::new(
            "special",
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets::blobs(Hash::from_bytes([1u8; 32]), None),
            [0u8; 32],
            [0u8; 32],
            [0u8; 32],
            ResultDeliveryMode::Sync,
        );
        let result = executor.execute(special_request).await.unwrap();
        assert_eq!(result.exit_code, 42);

        // Other jobs fail
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
        // Verify JobExecutor can be used as a trait object
        let executor: Box<dyn JobExecutor> = Box::new(MockJobExecutor::success());
        let request = make_test_request("trait-object-test");
        let result = executor.execute(request).await;
        assert!(result.is_ok());
    }
}
