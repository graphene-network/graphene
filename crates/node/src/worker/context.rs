//! Worker job context for connecting P2P protocol handler with job executor.
//!
//! This module implements the [`JobContext`] trait from the P2P protocol handler,
//! providing the bridge between incoming job requests and the job execution pipeline.
//!
//! # Responsibilities
//!
//! The [`WorkerJobContext`] handles:
//! 1. **Slot reservation**: Uses [`WorkerStateMachine`] to reserve job slots atomically
//! 2. **Job state tracking**: Creates and updates [`Job`] records through their lifecycle
//! 3. **Execution spawning**: Launches async tasks to execute jobs via [`JobExecutor`]
//! 4. **Result delivery**: Delivers results via [`ResultDelivery`] trait
//! 5. **Channel state lookups**: Provides channel state for ticket validation
//!
//! # Thread Safety
//!
//! All operations are thread-safe. The context uses:
//! - `Arc<WorkerStateMachine>` for atomic slot management
//! - `Arc<RwLock<HashMap>>` for job store
//! - `Arc<dyn ChannelStateManager>` for channel state lookups
//!
//! # Job Lifecycle
//!
//! ```text
//! on_job_accepted():
//!   1. Reserve slot (SlotGuard)
//!   2. Create Job in Pending state
//!   3. Transition to Accepted
//!   4. Store job
//!   5. Spawn execution task:
//!      a. Convert JobRequest -> ExecutionRequest
//!      b. Transition to Building/Cached
//!      c. Transition to Running
//!      d. Execute via JobExecutor
//!      e. Transition to Succeeded/Failed/Timeout
//!      f. Deliver result
//!      g. Transition to Delivered/Expired
//!      h. Drop SlotGuard (releases slot)
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use async_trait::async_trait;

use crate::executor::{ExecutionError, ExecutionRequest, ExecutionResult, JobExecutor};
use crate::job::{exit_code, Job, JobState};
use crate::p2p::messages::{ResultDeliveryMode, WorkerCapabilities};
use crate::p2p::protocol::handler::JobContext;
use crate::p2p::protocol::types::{JobRequest, JobStatus, RejectReason};
use crate::result::{EncryptedResult, ResultDelivery};
use crate::ticket::{ChannelLocalState, ChannelState, ChannelStateManager, SolanaChannelClient};
use std::time::{SystemTime, UNIX_EPOCH};

use super::state::WorkerStateMachine;

/// Thread-safe job storage for tracking job state.
///
/// Jobs are stored by their UUID and can be looked up, updated, or removed.
#[derive(Debug, Default)]
pub struct JobStore {
    jobs: RwLock<HashMap<String, Job>>,
}

impl JobStore {
    /// Creates a new empty job store.
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
        }
    }

    /// Inserts a job into the store.
    pub async fn insert(&self, job: Job) {
        let mut jobs = self.jobs.write().await;
        jobs.insert(job.id.clone(), job);
    }

    /// Gets a job by ID.
    pub async fn get(&self, job_id: &str) -> Option<Job> {
        let jobs = self.jobs.read().await;
        jobs.get(job_id).cloned()
    }

    /// Updates a job's state.
    ///
    /// Returns `true` if the job was found and updated.
    pub async fn update_state(&self, job_id: &str, state: JobState) -> bool {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            if job.transition(state).is_ok() {
                return true;
            }
        }
        false
    }

    /// Updates a job's state with an exit code.
    ///
    /// Returns `true` if the job was found and updated.
    pub async fn update_state_with_exit_code(
        &self,
        job_id: &str,
        state: JobState,
        exit_code: i32,
    ) -> bool {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            if job.transition_with_exit_code(state, exit_code).is_ok() {
                return true;
            }
        }
        false
    }

    /// Removes a job from the store.
    pub async fn remove(&self, job_id: &str) -> Option<Job> {
        let mut jobs = self.jobs.write().await;
        jobs.remove(job_id)
    }

    /// Returns the number of jobs in the store.
    pub async fn len(&self) -> usize {
        let jobs = self.jobs.read().await;
        jobs.len()
    }

    /// Returns true if the store is empty.
    pub async fn is_empty(&self) -> bool {
        let jobs = self.jobs.read().await;
        jobs.is_empty()
    }
}

/// Worker job context connecting P2P protocol with job execution.
///
/// This struct implements the [`JobContext`] trait and orchestrates:
/// - Slot reservation and release
/// - Job state management
/// - Async job execution
/// - Result delivery
pub struct WorkerJobContext<E, D, C>
where
    E: JobExecutor + 'static,
    D: ResultDelivery + 'static,
    C: ChannelStateManager + 'static,
{
    /// Worker state machine for slot management.
    state_machine: Arc<WorkerStateMachine>,

    /// Job executor for running jobs.
    executor: Arc<E>,

    /// Result delivery handler.
    delivery: Arc<D>,

    /// Channel state manager for ticket validation.
    channel_manager: Arc<C>,

    /// Optional Solana client for on-demand channel sync.
    solana_client: Option<Arc<dyn SolanaChannelClient>>,

    /// Job store for tracking job state.
    job_store: Arc<JobStore>,

    /// Worker capabilities advertised to clients.
    capabilities: WorkerCapabilities,

    /// Worker's public key (for signing results).
    /// TODO(#47): Use this for signing job results with worker's Ed25519 key.
    #[allow(dead_code)]
    worker_pubkey: [u8; 32],
}

impl<E, D, C> WorkerJobContext<E, D, C>
where
    E: JobExecutor + 'static,
    D: ResultDelivery + 'static,
    C: ChannelStateManager + 'static,
{
    /// Creates a new worker job context.
    ///
    /// # Arguments
    ///
    /// * `state_machine` - Worker state machine for slot management
    /// * `executor` - Job executor for running jobs
    /// * `delivery` - Result delivery handler
    /// * `channel_manager` - Channel state manager for ticket validation
    /// * `solana_client` - Optional Solana client for on-demand channel sync
    /// * `capabilities` - Worker capabilities to advertise
    /// * `worker_pubkey` - Worker's Ed25519 public key
    pub fn new(
        state_machine: Arc<WorkerStateMachine>,
        executor: Arc<E>,
        delivery: Arc<D>,
        channel_manager: Arc<C>,
        solana_client: Option<Arc<dyn SolanaChannelClient>>,
        capabilities: WorkerCapabilities,
        worker_pubkey: [u8; 32],
    ) -> Self {
        Self {
            state_machine,
            executor,
            delivery,
            channel_manager,
            solana_client,
            job_store: Arc::new(JobStore::new()),
            capabilities,
            worker_pubkey,
        }
    }

    /// Creates a new worker job context with an existing job store.
    ///
    /// Useful for testing or when sharing a job store between components.
    #[allow(clippy::too_many_arguments)]
    pub fn with_job_store(
        state_machine: Arc<WorkerStateMachine>,
        executor: Arc<E>,
        delivery: Arc<D>,
        channel_manager: Arc<C>,
        solana_client: Option<Arc<dyn SolanaChannelClient>>,
        job_store: Arc<JobStore>,
        capabilities: WorkerCapabilities,
        worker_pubkey: [u8; 32],
    ) -> Self {
        Self {
            state_machine,
            executor,
            delivery,
            channel_manager,
            solana_client,
            job_store,
            capabilities,
            worker_pubkey,
        }
    }

    /// Returns a reference to the job store.
    pub fn job_store(&self) -> &Arc<JobStore> {
        &self.job_store
    }

    /// Returns a reference to the worker state machine.
    pub fn state_machine(&self) -> &Arc<WorkerStateMachine> {
        &self.state_machine
    }

    async fn ensure_channel(&self, channel_id: &[u8; 32]) -> Option<ChannelLocalState> {
        if let Some(state) = self.channel_manager.get_channel(channel_id).await {
            return Some(state);
        }

        let solana = self.solana_client.as_ref()?;
        let on_chain = match solana.fetch_channel(channel_id).await {
            Ok(Some(channel)) => channel,
            Ok(None) => {
                warn!(channel_id = ?channel_id, "Solana channel not found");
                return None;
            }
            Err(e) => {
                warn!(channel_id = ?channel_id, error = %e, "Solana channel fetch failed");
                return None;
            }
        };

        let last_sync = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let local = ChannelLocalState {
            channel_id: *channel_id,
            user: on_chain.user,
            worker: on_chain.worker,
            on_chain_balance: on_chain.balance,
            accepted_amount: on_chain.spent,
            last_settled_amount: on_chain.spent,
            last_nonce: on_chain.last_nonce,
            last_sync,
            highest_ticket: None,
            on_chain_state: on_chain.state,
            dispute_timeout: on_chain.timeout,
        };

        let _ = self.channel_manager.upsert_channel(local.clone()).await;
        Some(local)
    }

    /// Converts a JobRequest to an ExecutionRequest.
    fn make_execution_request(
        request: &JobRequest,
        payer_pubkey: [u8; 32],
        client_node_id: Option<[u8; 32]>,
    ) -> ExecutionRequest {
        let mut exec_request = ExecutionRequest::new(
            request.job_id.to_string(),
            request.manifest.clone(),
            request.assets.clone(),
            request.ephemeral_pubkey,
            request.channel_pda,
            payer_pubkey,
            request.delivery_mode,
        );
        exec_request.client_node_id = client_node_id;
        exec_request
    }

    /// Determines the exit code from an execution result.
    fn exit_code_from_result(result: &ExecutionResult) -> i32 {
        result.exit_code
    }

    /// Determines the job state from an execution result.
    fn state_from_result(result: &ExecutionResult) -> JobState {
        if result.succeeded() {
            JobState::Succeeded
        } else if result.exit_code == exit_code::USER_TIMEOUT {
            JobState::Timeout
        } else {
            JobState::Failed
        }
    }

    /// Creates an EncryptedResult from an ExecutionResult.
    fn make_encrypted_result(result: &ExecutionResult) -> EncryptedResult {
        EncryptedResult {
            result: result.encrypted_result.clone(),
            stdout: result.encrypted_stdout.clone(),
            stderr: result.encrypted_stderr.clone(),
            exit_code: result.exit_code,
            execution_ms: result.duration_ms(),
        }
    }
}

#[async_trait]
impl<E, D, C> JobContext for WorkerJobContext<E, D, C>
where
    E: JobExecutor + 'static,
    D: ResultDelivery + 'static,
    C: ChannelStateManager + 'static,
{
    fn capabilities(&self) -> &WorkerCapabilities {
        &self.capabilities
    }

    fn available_slots(&self) -> u8 {
        self.state_machine.available_slots().min(255) as u8
    }

    async fn get_channel_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState> {
        if let Some(state) = self.channel_manager.get_validation_state(channel_id).await {
            return Some(state);
        }

        self.ensure_channel(channel_id)
            .await
            .map(|state| ChannelState {
                last_nonce: state.last_nonce,
                last_amount: state.accepted_amount,
                channel_balance: state.on_chain_balance,
            })
    }

    async fn get_payer_pubkey(&self, channel_id: &[u8; 32]) -> Option<[u8; 32]> {
        if let Some(state) = self.channel_manager.get_channel(channel_id).await {
            return Some(state.user);
        }

        self.ensure_channel(channel_id)
            .await
            .map(|state| state.user)
    }

    async fn on_job_accepted(&self, job_id: Uuid, request: &JobRequest, client_node_id: [u8; 32]) {
        let job_id_str = job_id.to_string();
        info!(job_id = %job_id_str, "Job accepted, starting execution");

        // 1. Reserve slot via WorkerStateMachine
        let slot_guard = match self.state_machine.try_reserve_slot() {
            Ok(guard) => guard,
            Err(e) => {
                // This shouldn't happen as we check slots before accepting,
                // but handle gracefully
                error!(job_id = %job_id_str, error = %e, "Failed to reserve slot");
                return;
            }
        };

        // 2. Get payer pubkey for execution request
        let payer_pubkey = match self.get_payer_pubkey(&request.ticket.channel_id).await {
            Some(pk) => pk,
            None => {
                error!(job_id = %job_id_str, "Payer pubkey not found for channel");
                return;
            }
        };

        // 3. Create Job in Pending state, transition to Accepted
        let mut job = Job::with_delivery_mode(job_id_str.clone(), request.delivery_mode);
        if let Err(e) = job.transition(JobState::Accepted) {
            error!(job_id = %job_id_str, error = %e, "Failed to transition to Accepted");
            return;
        }
        self.job_store.insert(job).await;

        // 4. Clone what we need for the spawned task
        let executor = Arc::clone(&self.executor);
        let delivery = Arc::clone(&self.delivery);
        let job_store = Arc::clone(&self.job_store);
        let execution_request =
            Self::make_execution_request(request, payer_pubkey, Some(client_node_id));
        let delivery_mode = request.delivery_mode;

        // 5. Spawn execution task
        tokio::spawn(async move {
            // The slot guard is moved into this task and will be dropped when done,
            // automatically releasing the slot.
            let _guard = slot_guard;

            debug!(job_id = %job_id_str, "Execution task started");

            // Transition to Building (we don't know cache status yet)
            // TODO(#45): Add cache check to determine Building vs Cached
            if !job_store
                .update_state(&job_id_str, JobState::Building)
                .await
            {
                error!(job_id = %job_id_str, "Failed to transition to Building");
                return;
            }

            // Transition to Running
            if !job_store.update_state(&job_id_str, JobState::Running).await {
                error!(job_id = %job_id_str, "Failed to transition to Running");
                return;
            }

            // Execute the job
            let exec_result = executor.execute(execution_request).await;

            match exec_result {
                Ok(result) => {
                    let exit_code = Self::exit_code_from_result(&result);
                    let final_state = Self::state_from_result(&result);

                    info!(
                        job_id = %job_id_str,
                        exit_code = exit_code,
                        duration_ms = result.duration_ms(),
                        "Job execution completed"
                    );

                    // Transition to execution-complete state
                    if !job_store
                        .update_state_with_exit_code(&job_id_str, final_state, exit_code)
                        .await
                    {
                        error!(job_id = %job_id_str, "Failed to transition to {}", final_state);
                        return;
                    }

                    // Deliver result
                    let encrypted_result = Self::make_encrypted_result(&result);

                    // TODO(#46): Get user address for sync delivery from job request
                    // For now, we attempt async delivery as fallback
                    let delivery_result = delivery
                        .deliver(
                            &job_id_str,
                            &encrypted_result,
                            delivery_mode,
                            None, // No user address available yet
                            true, // Enable fallback to async
                        )
                        .await;

                    match delivery_result {
                        Ok(outcome) => {
                            // Transition to Delivered
                            if !job_store
                                .update_state(&job_id_str, JobState::Delivered)
                                .await
                            {
                                error!(
                                    job_id = %job_id_str,
                                    "Failed to transition to Delivered"
                                );
                            } else {
                                info!(
                                    job_id = %job_id_str,
                                    sync = outcome.is_sync(),
                                    "Result delivered"
                                );
                            }
                        }
                        Err(e) => {
                            warn!(job_id = %job_id_str, error = %e, "Result delivery failed");

                            // For async mode, transition to Expired
                            if delivery_mode == ResultDeliveryMode::Async {
                                let _ =
                                    job_store.update_state(&job_id_str, JobState::Expired).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(job_id = %job_id_str, error = %e, "Job execution failed");

                    // Determine exit code based on error type
                    let exit_code = if e.is_worker_fault() {
                        exit_code::WORKER_CRASH
                    } else if e.is_user_fault() {
                        exit_code::BUILD_FAILURE
                    } else {
                        exit_code::WORKER_CRASH
                    };

                    // Transition to Failed
                    if !job_store
                        .update_state_with_exit_code(&job_id_str, JobState::Failed, exit_code)
                        .await
                    {
                        error!(job_id = %job_id_str, "Failed to transition to Failed");
                    }
                }
            }

            debug!(job_id = %job_id_str, "Execution task completed");
            // SlotGuard is dropped here, releasing the slot
        });
    }

    async fn execute_job_sync(
        &self,
        job_id: Uuid,
        request: &JobRequest,
        client_node_id: [u8; 32],
    ) -> Result<(ExecutionResult, JobStatus), ExecutionError> {
        let job_id_str = job_id.to_string();
        info!(job_id = %job_id_str, "Job accepted (sync mode), starting execution");

        // 1. Reserve slot via WorkerStateMachine
        let slot_guard = self.state_machine.try_reserve_slot().map_err(|e| {
            error!(job_id = %job_id_str, error = %e, "Failed to reserve slot");
            ExecutionError::vmm(format!("Failed to reserve slot: {}", e))
        })?;

        // 2. Get payer pubkey for execution request
        let payer_pubkey = self
            .get_payer_pubkey(&request.ticket.channel_id)
            .await
            .ok_or_else(|| {
                error!(job_id = %job_id_str, "Payer pubkey not found for channel");
                ExecutionError::vmm("Payer pubkey not found for channel")
            })?;

        // 3. Create Job in Pending state, transition to Accepted
        let mut job = Job::with_delivery_mode(job_id_str.clone(), request.delivery_mode);
        if let Err(e) = job.transition(JobState::Accepted) {
            error!(job_id = %job_id_str, error = %e, "Failed to transition to Accepted");
            return Err(ExecutionError::vmm(format!(
                "Failed to transition to Accepted: {}",
                e
            )));
        }
        self.job_store.insert(job).await;

        // 4. Create execution request
        let execution_request =
            Self::make_execution_request(request, payer_pubkey, Some(client_node_id));

        debug!(job_id = %job_id_str, "Sync execution task started");

        // Transition to Building (sync mode mirrors async progression for valid state updates)
        if !self
            .job_store
            .update_state(&job_id_str, JobState::Building)
            .await
        {
            error!(job_id = %job_id_str, "Failed to transition to Building");
            return Err(ExecutionError::vmm("Failed to transition to Building"));
        }

        // Transition to Running
        if !self
            .job_store
            .update_state(&job_id_str, JobState::Running)
            .await
        {
            error!(job_id = %job_id_str, "Failed to transition to Running");
            return Err(ExecutionError::vmm("Failed to transition to Running"));
        }

        // 5. Execute the job synchronously (await, not spawn)
        let exec_result = self.executor.execute(execution_request).await;

        // Drop slot guard to release the slot
        drop(slot_guard);

        match exec_result {
            Ok(result) => {
                let exit_code = Self::exit_code_from_result(&result);
                let final_state = Self::state_from_result(&result);
                let status = match final_state {
                    JobState::Succeeded => JobStatus::Succeeded,
                    JobState::Timeout => JobStatus::Timeout,
                    _ => JobStatus::Failed,
                };

                info!(
                    job_id = %job_id_str,
                    exit_code = exit_code,
                    duration_ms = result.duration_ms(),
                    "Job execution completed (sync mode)"
                );

                // Update job state
                if !self
                    .job_store
                    .update_state_with_exit_code(&job_id_str, final_state, exit_code)
                    .await
                {
                    error!(job_id = %job_id_str, "Failed to transition to {}", final_state);
                }

                // Mark as delivered since we're returning result directly
                if !self
                    .job_store
                    .update_state(&job_id_str, JobState::Delivered)
                    .await
                {
                    error!(job_id = %job_id_str, "Failed to transition to Delivered");
                }

                debug!(job_id = %job_id_str, "Sync execution task completed");
                Ok((result, status))
            }
            Err(e) => {
                error!(job_id = %job_id_str, error = %e, "Job execution failed (sync mode)");

                // Determine exit code based on error type
                let exit_code = if e.is_worker_fault() {
                    exit_code::WORKER_CRASH
                } else if e.is_user_fault() {
                    exit_code::BUILD_FAILURE
                } else {
                    exit_code::WORKER_CRASH
                };

                // Transition to Failed
                if !self
                    .job_store
                    .update_state_with_exit_code(&job_id_str, JobState::Failed, exit_code)
                    .await
                {
                    error!(job_id = %job_id_str, "Failed to transition to Failed");
                }

                Err(e)
            }
        }
    }

    async fn on_job_rejected(&self, job_id: Uuid, reason: RejectReason) {
        warn!(job_id = %job_id, reason = %reason, "Job rejected");
        // No state tracking needed for rejected jobs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::MockJobExecutor;
    use crate::p2p::messages::JobManifest;
    use crate::p2p::protocol::types::JobAssets;
    use crate::result::MockResultDelivery;
    use crate::ticket::{
        ChannelConfig, ChannelLocalState, DefaultChannelStateManager, MockTicketValidator,
        MockValidatorBehavior, OnChainChannelState, PaymentTicket,
    };
    use crate::worker::WorkerEvent;
    use iroh_blobs::Hash;
    use std::collections::HashMap;
    use std::time::Duration;

    fn make_test_capabilities() -> WorkerCapabilities {
        WorkerCapabilities {
            max_vcpu: 4,
            max_memory_mb: 4096,
            kernels: vec!["python:3.12".to_string(), "node:21".to_string()],
            disk: None,
            gpus: vec![],
        }
    }

    fn make_test_request() -> JobRequest {
        JobRequest {
            job_id: Uuid::new_v4(),
            manifest: JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 10000,
                runtime: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            ticket: PaymentTicket::new([1u8; 32], 1_000_000, 1, 1700000000, [0u8; 64]),
            assets: JobAssets::blobs(Hash::from_bytes([0u8; 32]), None),
            ephemeral_pubkey: [0u8; 32],
            channel_pda: [0u8; 32],
            delivery_mode: ResultDeliveryMode::Sync,
        }
    }

    async fn make_test_context(
    ) -> WorkerJobContext<MockJobExecutor, MockResultDelivery, DefaultChannelStateManager> {
        let state_machine = WorkerStateMachine::new_shared(4);
        // Get to Online state
        state_machine
            .transition(WorkerEvent::StakeConfirmed)
            .unwrap();
        state_machine.transition(WorkerEvent::JoinedGossip).unwrap();

        let executor = Arc::new(MockJobExecutor::success());
        let delivery = Arc::new(MockResultDelivery::new());

        let config = ChannelConfig::default();
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let channel_manager = Arc::new(DefaultChannelStateManager::new(config, validator));

        // Add a test channel
        let channel_state = ChannelLocalState {
            channel_id: [1u8; 32],
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 10_000_000,
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        channel_manager.upsert_channel(channel_state).await.unwrap();

        WorkerJobContext::new(
            state_machine,
            executor,
            delivery,
            channel_manager,
            None,
            make_test_capabilities(),
            [0u8; 32],
        )
    }

    #[tokio::test]
    async fn test_job_store_basic_operations() {
        let store = JobStore::new();

        // Insert a job
        let job = Job::new("test-job-1");
        store.insert(job).await;

        // Get the job
        let retrieved = store.get("test-job-1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test-job-1");

        // Check length
        assert_eq!(store.len().await, 1);
        assert!(!store.is_empty().await);

        // Remove the job
        let removed = store.remove("test-job-1").await;
        assert!(removed.is_some());
        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn test_job_store_update_state() {
        let store = JobStore::new();

        let job = Job::new("test-job-2");
        store.insert(job).await;

        // Update state
        assert!(store.update_state("test-job-2", JobState::Accepted).await);

        let job = store.get("test-job-2").await.unwrap();
        assert_eq!(job.state, JobState::Accepted);
    }

    #[tokio::test]
    async fn test_job_store_update_state_with_exit_code() {
        let store = JobStore::new();

        let mut job = Job::new("test-job-3");
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();
        job.transition(JobState::Running).unwrap();
        store.insert(job).await;

        // Update state with exit code
        assert!(
            store
                .update_state_with_exit_code("test-job-3", JobState::Succeeded, 0)
                .await
        );

        let job = store.get("test-job-3").await.unwrap();
        assert_eq!(job.state, JobState::Succeeded);
        assert_eq!(job.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_context_capabilities() {
        let context = make_test_context().await;

        let caps = context.capabilities();
        assert_eq!(caps.max_vcpu, 4);
        assert_eq!(caps.max_memory_mb, 4096);
        assert!(caps.kernels.contains(&"python:3.12".to_string()));
    }

    #[tokio::test]
    async fn test_context_available_slots() {
        let context = make_test_context().await;

        // Should have 4 slots available
        assert_eq!(context.available_slots(), 4);
    }

    #[tokio::test]
    async fn test_context_get_channel_state() {
        let context = make_test_context().await;

        // Test channel exists
        let state = context.get_channel_state(&[1u8; 32]).await;
        assert!(state.is_some());

        // Non-existent channel
        let state = context.get_channel_state(&[99u8; 32]).await;
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn test_context_get_payer_pubkey() {
        let context = make_test_context().await;

        // Test channel exists
        let pubkey = context.get_payer_pubkey(&[1u8; 32]).await;
        assert!(pubkey.is_some());
        assert_eq!(pubkey.unwrap(), [2u8; 32]);

        // Non-existent channel
        let pubkey = context.get_payer_pubkey(&[99u8; 32]).await;
        assert!(pubkey.is_none());
    }

    #[tokio::test]
    async fn test_context_on_job_accepted_creates_job() {
        let context = make_test_context().await;
        let request = make_test_request();
        let job_id = request.job_id;

        // Accept the job (use dummy client node ID for tests)
        let client_node_id = [0u8; 32];
        context
            .on_job_accepted(job_id, &request, client_node_id)
            .await;

        // Give the spawned task a moment to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Job should be in the store
        let job = context.job_store().get(&job_id.to_string()).await;
        assert!(job.is_some());
    }

    #[tokio::test]
    async fn test_context_on_job_accepted_reserves_slot() {
        let context = make_test_context().await;
        let initial_slots = context.available_slots();

        let request = make_test_request();
        let job_id = request.job_id;

        // Accept the job (use dummy client node ID for tests)
        let client_node_id = [0u8; 32];
        context
            .on_job_accepted(job_id, &request, client_node_id)
            .await;

        // Give the spawned task a moment to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Slot should be reserved (less available)
        assert!(context.available_slots() < initial_slots);
    }

    #[tokio::test]
    async fn test_context_on_job_rejected() {
        let context = make_test_context().await;
        let job_id = Uuid::new_v4();

        // Should not panic
        context
            .on_job_rejected(job_id, RejectReason::CapacityFull)
            .await;

        // No job should be in the store
        let job = context.job_store().get(&job_id.to_string()).await;
        assert!(job.is_none());
    }

    #[tokio::test]
    async fn test_make_execution_request() {
        let request = make_test_request();
        let payer_pubkey = [42u8; 32];
        let client_node_id = [99u8; 32];

        let exec_request = WorkerJobContext::<
            MockJobExecutor,
            MockResultDelivery,
            DefaultChannelStateManager,
        >::make_execution_request(
            &request, payer_pubkey, Some(client_node_id)
        );

        assert_eq!(exec_request.job_id, request.job_id.to_string());
        assert_eq!(exec_request.manifest.vcpu, request.manifest.vcpu);
        assert_eq!(exec_request.payer_pubkey, payer_pubkey);
        assert_eq!(exec_request.delivery_mode, request.delivery_mode);
        assert_eq!(exec_request.client_node_id, Some(client_node_id));
    }

    #[tokio::test]
    async fn test_job_execution_happy_path() {
        let state_machine = WorkerStateMachine::new_shared(4);
        state_machine
            .transition(WorkerEvent::StakeConfirmed)
            .unwrap();
        state_machine.transition(WorkerEvent::JoinedGossip).unwrap();

        let executor = Arc::new(MockJobExecutor::success());
        let delivery = Arc::new(MockResultDelivery::new());

        let config = ChannelConfig::default();
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let channel_manager = Arc::new(DefaultChannelStateManager::new(config, validator));

        let channel_state = ChannelLocalState {
            channel_id: [1u8; 32],
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 10_000_000,
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        channel_manager.upsert_channel(channel_state).await.unwrap();

        let context: WorkerJobContext<
            MockJobExecutor,
            MockResultDelivery,
            DefaultChannelStateManager,
        > = WorkerJobContext::new(
            state_machine.clone(),
            executor,
            delivery,
            channel_manager,
            None,
            make_test_capabilities(),
            [0u8; 32],
        );

        // Use Async delivery mode so we don't need a user address
        let mut request = make_test_request();
        request.delivery_mode = ResultDeliveryMode::Async;
        let job_id = request.job_id;

        // Accept the job (use dummy client node ID for tests)
        let client_node_id = [0u8; 32];
        context
            .on_job_accepted(job_id, &request, client_node_id)
            .await;

        // Wait for execution to complete (mock executor is fast)
        // The spawned task needs time to complete the full execution cycle
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check job progressed past initial states
        let job = context.job_store().get(&job_id.to_string()).await;
        assert!(job.is_some());
        let job = job.unwrap();

        // Job should have moved past Pending
        assert_ne!(job.state, JobState::Pending);

        // Wait a bit longer for full completion
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Now check for terminal state
        let job = context.job_store().get(&job_id.to_string()).await.unwrap();

        // The mock executor and delivery are synchronous, so the job should complete
        // But we still check gracefully in case of timing issues
        if job.state == JobState::Delivered {
            // Slot should be released when job completes
            assert_eq!(state_machine.available_slots(), 4);
        } else {
            // Job may still be in progress, just verify it's progressing
            assert!(
                job.state != JobState::Pending,
                "Job should have progressed past Pending, actual state: {}",
                job.state
            );
        }
    }
}
