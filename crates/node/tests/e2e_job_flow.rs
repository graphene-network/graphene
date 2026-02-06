//! End-to-end integration tests for the complete job execution flow.
//!
//! These tests verify the full job lifecycle from submission through result delivery,
//! testing state transitions and proper coordination between components.
//!
//! Run with: `cargo test --features integration-tests --test e2e_job_flow`

#![cfg(feature = "integration-tests")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use uuid::Uuid;

use graphene_node::executor::{MockExecutorBehavior, MockJobExecutor};
use graphene_node::job::JobState;
use graphene_node::p2p::messages::{JobManifest, ResultDeliveryMode, WorkerCapabilities};
use graphene_node::p2p::protocol::types::{AssetData, Compression, JobAssets};
use graphene_node::p2p::protocol::{JobContext, JobRequest};
use graphene_node::result::mock::MockDeliveryBehavior;
use graphene_node::result::MockResultDelivery;
use graphene_node::ticket::{
    ChannelConfig, ChannelLocalState, ChannelStateManager, DefaultChannelStateManager,
    MockTicketValidator, MockValidatorBehavior, OnChainChannelState, PaymentTicket,
};
use graphene_node::worker::{JobStore, WorkerEvent, WorkerJobContext, WorkerStateMachine};

// ============================================================================
// Test Helpers
// ============================================================================

/// Helper to observe state transitions by polling the job store.
struct StateObserver {
    transitions: Arc<Mutex<Vec<JobState>>>,
    last_state: Arc<Mutex<Option<JobState>>>,
}

impl StateObserver {
    fn new() -> Self {
        Self {
            transitions: Arc::new(Mutex::new(Vec::new())),
            last_state: Arc::new(Mutex::new(None)),
        }
    }

    /// Record a new state if it's different from the last observed state.
    async fn record(&self, state: JobState) {
        let mut last = self.last_state.lock().await;
        if *last != Some(state) {
            self.transitions.lock().await.push(state);
            *last = Some(state);
        }
    }

    /// Get all recorded state transitions.
    async fn transitions(&self) -> Vec<JobState> {
        self.transitions.lock().await.clone()
    }
}

/// Wait for a job to reach a specific state, with timeout.
#[allow(dead_code)] // Used for specific state assertions in some tests
async fn wait_for_state(
    job_store: &JobStore,
    job_id: &str,
    expected: JobState,
    timeout_ms: u64,
) -> Result<(), String> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if let Some(job) = job_store.get(job_id).await {
            if job.state == expected {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Get final state for error message
    let final_state = job_store
        .get(job_id)
        .await
        .map(|j| j.state)
        .map(|s| format!("{:?}", s))
        .unwrap_or_else(|| "not found".to_string());

    Err(format!(
        "Timed out waiting for state {:?}, final state: {}",
        expected, final_state
    ))
}

/// Wait for a job to reach any terminal state.
async fn wait_for_terminal(
    job_store: &JobStore,
    job_id: &str,
    timeout_ms: u64,
) -> Result<JobState, String> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if let Some(job) = job_store.get(job_id).await {
            if job.state.is_terminal() || job.state.is_execution_complete() {
                return Ok(job.state);
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let final_state = job_store
        .get(job_id)
        .await
        .map(|j| j.state)
        .map(|s| format!("{:?}", s))
        .unwrap_or_else(|| "not found".to_string());

    Err(format!(
        "Timed out waiting for terminal state, final state: {}",
        final_state
    ))
}

/// Observe state transitions of a job until it reaches a terminal state.
async fn observe_states(
    job_store: &JobStore,
    job_id: &str,
    timeout_ms: u64,
) -> Result<Vec<JobState>, String> {
    let observer = StateObserver::new();
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if let Some(job) = job_store.get(job_id).await {
            observer.record(job.state).await;

            if job.state.is_terminal() {
                return Ok(observer.transitions().await);
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    Err(format!(
        "Timed out observing states, observed: {:?}",
        observer.transitions().await
    ))
}

fn make_test_capabilities() -> WorkerCapabilities {
    WorkerCapabilities {
        max_vcpu: 4,
        max_memory_mb: 4096,
        kernels: vec!["python:3.12".to_string(), "node:20".to_string()],
        disk: None,
        gpus: vec![],
    }
}

fn make_test_request(delivery_mode: ResultDeliveryMode) -> JobRequest {
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
        assets: JobAssets {
            code: AssetData::Blob {
                hash: iroh_blobs::Hash::from_bytes([1u8; 32]),
                url: None,
            },
            input: Some(AssetData::Blob {
                hash: iroh_blobs::Hash::from_bytes([2u8; 32]),
                url: None,
            }),
            files: vec![],
            compression: Compression::None,
        },
        ephemeral_pubkey: [0u8; 32],
        channel_pda: [1u8; 32],
        delivery_mode,
    }
}

async fn make_test_context(
    executor_behavior: MockExecutorBehavior,
    delivery_behavior: MockDeliveryBehavior,
) -> (
    WorkerJobContext<MockJobExecutor, MockResultDelivery, DefaultChannelStateManager>,
    Arc<JobStore>,
) {
    let state_machine = WorkerStateMachine::new_shared(4);
    // Transition to Online state
    state_machine
        .transition(WorkerEvent::StakeConfirmed)
        .unwrap();
    state_machine.transition(WorkerEvent::JoinedGossip).unwrap();

    let executor = Arc::new(MockJobExecutor::new(executor_behavior));
    let delivery = Arc::new(MockResultDelivery::with_behavior(delivery_behavior));

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

    let job_store = Arc::new(JobStore::new());

    let context = WorkerJobContext::with_job_store(
        state_machine,
        executor,
        delivery,
        channel_manager,
        None,
        job_store.clone(),
        make_test_capabilities(),
        [0u8; 32],
    );

    (context, job_store)
}

// ============================================================================
// E2E Tests
// ============================================================================

/// Test the complete happy path from job submission to result delivery.
///
/// Verifies state transitions: Pending -> Accepted -> Building -> Running -> Succeeded -> Delivered
#[tokio::test]
async fn test_e2e_job_submission_to_delivery() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(50),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    // Use Async delivery mode so we don't need user address
    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    // Accept the job
    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Wait for terminal state
    let final_state = wait_for_terminal(&job_store, &job_id_str, 2000)
        .await
        .expect("Job should complete");

    assert_eq!(
        final_state,
        JobState::Delivered,
        "Job should end in Delivered state"
    );

    // Verify the job was stored and has correct final state
    let job = job_store.get(&job_id_str).await.expect("Job should exist");
    assert_eq!(job.state, JobState::Delivered);
    assert_eq!(job.exit_code, Some(0));

    // Verify state history progressed correctly
    assert!(
        job.state_history.len() >= 5,
        "Should have multiple state transitions"
    );

    // Verify the state history contains expected states
    let states: Vec<_> = job.state_history.iter().map(|t| t.state).collect();
    assert!(
        states.contains(&JobState::Pending),
        "Should have Pending state"
    );
    assert!(
        states.contains(&JobState::Accepted),
        "Should have Accepted state"
    );
    assert!(
        states.contains(&JobState::Building),
        "Should have Building state"
    );
    assert!(
        states.contains(&JobState::Running),
        "Should have Running state"
    );
    assert!(
        states.contains(&JobState::Succeeded),
        "Should have Succeeded state"
    );
    assert!(
        states.contains(&JobState::Delivered),
        "Should have Delivered state"
    );
}

/// Test that slots are released after job completion.
#[tokio::test]
async fn test_e2e_slot_release_after_completion() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(20),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let initial_slots = context.available_slots();
    assert_eq!(initial_slots, 4);

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    // Accept the job
    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Give the spawned task time to reserve the slot
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Slot should be reserved during execution
    assert!(
        context.available_slots() < initial_slots,
        "Slot should be reserved"
    );

    // Wait for completion
    wait_for_terminal(&job_store, &job_id_str, 2000)
        .await
        .expect("Job should complete");

    // Slot should be released
    assert_eq!(
        context.available_slots(),
        initial_slots,
        "Slot should be released after completion"
    );
}

/// Test job failure flow - execution fails with non-zero exit code.
#[tokio::test]
async fn test_e2e_job_failure_user_error() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 1, // User error exit code
            duration: Duration::from_millis(20),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Wait for terminal state
    let final_state = wait_for_terminal(&job_store, &job_id_str, 2000)
        .await
        .expect("Job should complete");

    // Even with non-zero exit code, execution completed
    // The job should still be delivered (with error info)
    assert!(
        matches!(final_state, JobState::Delivered | JobState::Failed),
        "Job should reach Delivered or Failed, got {:?}",
        final_state
    );

    let job = job_store.get(&job_id_str).await.expect("Job should exist");
    assert_eq!(job.exit_code, Some(1));
}

/// Test job failure flow - executor returns an error.
#[tokio::test]
async fn test_e2e_job_failure_executor_error() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Failure("VM crashed".to_string()),
        MockDeliveryBehavior::Success,
    )
    .await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Wait for the job to reach a terminal/error state
    tokio::time::sleep(Duration::from_millis(500)).await;

    let job = job_store.get(&job_id_str).await.expect("Job should exist");

    // Job should be in Failed state due to executor error
    assert_eq!(
        job.state,
        JobState::Failed,
        "Job should be Failed after executor error"
    );
}

/// Test job timeout flow.
#[tokio::test]
async fn test_e2e_job_timeout_flow() {
    let (context, job_store) =
        make_test_context(MockExecutorBehavior::Timeout, MockDeliveryBehavior::Success).await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Wait for the job to fail due to timeout
    tokio::time::sleep(Duration::from_millis(500)).await;

    let job = job_store.get(&job_id_str).await.expect("Job should exist");

    // Job should be Failed (timeout error from executor)
    assert_eq!(
        job.state,
        JobState::Failed,
        "Job should be Failed after timeout error"
    );
}

/// Test delivery failure with fallback.
#[tokio::test]
async fn test_e2e_delivery_failure() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(20),
        },
        MockDeliveryBehavior::AlwaysFail,
    )
    .await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Wait for the job to reach a state
    tokio::time::sleep(Duration::from_millis(500)).await;

    let job = job_store.get(&job_id_str).await.expect("Job should exist");

    // Job execution completed successfully, but delivery failed
    // For async mode with failed delivery, should be Expired
    assert!(
        matches!(job.state, JobState::Expired | JobState::Succeeded),
        "Job should be Expired or Succeeded after delivery failure, got {:?}",
        job.state
    );
}

/// Test multiple concurrent jobs.
#[tokio::test]
async fn test_e2e_concurrent_jobs() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(30),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let mut job_ids = Vec::new();

    // Submit 3 concurrent jobs
    for _ in 0..3 {
        let request = make_test_request(ResultDeliveryMode::Async);
        let job_id = request.job_id;
        job_ids.push(job_id.to_string());
        context.on_job_accepted(job_id, &request, [0u8; 32]).await;
    }

    // Give time for all jobs to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // All jobs should be in progress or completed
    for job_id_str in &job_ids {
        let job = job_store.get(job_id_str).await.expect("Job should exist");
        assert_ne!(
            job.state,
            JobState::Pending,
            "Job should have progressed past Pending"
        );
    }

    // Wait for all to complete
    for job_id_str in &job_ids {
        wait_for_terminal(&job_store, job_id_str, 3000)
            .await
            .expect("Job should complete");
    }

    // All jobs should be delivered
    for job_id_str in &job_ids {
        let job = job_store.get(job_id_str).await.expect("Job should exist");
        assert_eq!(
            job.state,
            JobState::Delivered,
            "Job {} should be Delivered, got {:?}",
            job_id_str,
            job.state
        );
    }
}

/// Test state transition order is maintained.
#[tokio::test]
async fn test_e2e_state_transition_order() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(50),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Observe state transitions
    let states = observe_states(&job_store, &job_id_str, 3000)
        .await
        .expect("Should observe states");

    // Verify ordering - each state should come after its prerequisite
    let state_order = [
        JobState::Accepted,
        JobState::Building,
        JobState::Running,
        JobState::Succeeded,
        JobState::Delivered,
    ];

    let mut last_idx = 0;
    for expected in state_order {
        if let Some(idx) = states.iter().position(|&s| s == expected) {
            assert!(
                idx >= last_idx,
                "State {:?} should come after previous states, got order: {:?}",
                expected,
                states
            );
            last_idx = idx;
        }
        // Some states may be skipped in observation due to timing
    }
}

/// Test that job store tracks all jobs correctly.
#[tokio::test]
async fn test_e2e_job_store_tracking() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(20),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    assert!(job_store.is_empty().await, "Job store should start empty");

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    // Give the spawned task time to create the job
    tokio::time::sleep(Duration::from_millis(10)).await;

    assert!(!job_store.is_empty().await, "Job store should have the job");
    assert_eq!(job_store.len().await, 1, "Should have exactly one job");

    let job = job_store.get(&job_id_str).await.expect("Job should exist");
    assert_eq!(job.id, job_id_str);
}

/// Test job metrics are computed correctly.
#[tokio::test]
async fn test_e2e_job_metrics() {
    let (context, job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(50),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let request = make_test_request(ResultDeliveryMode::Async);
    let job_id = request.job_id;
    let job_id_str = job_id.to_string();

    context.on_job_accepted(job_id, &request, [0u8; 32]).await;

    wait_for_terminal(&job_store, &job_id_str, 3000)
        .await
        .expect("Job should complete");

    let job = job_store.get(&job_id_str).await.expect("Job should exist");
    let metrics = job.compute_metrics();

    // Queue time should be minimal since we immediately accept
    assert!(metrics.queue_ms.is_some(), "Should have queue time metric");

    // Boot time should be present (accepted -> running)
    assert!(metrics.boot_ms.is_some(), "Should have boot time metric");

    // Execution time should be present
    assert!(
        metrics.execution_ms.is_some(),
        "Should have execution time metric"
    );

    // Total time should be present
    assert!(metrics.total_ms.is_some(), "Should have total time metric");

    // Verify relationships
    let total = metrics.total_ms.unwrap();
    let boot = metrics.boot_ms.unwrap();
    let exec = metrics.execution_ms.unwrap();

    assert!(
        total >= boot + exec,
        "Total time should be >= boot + execution"
    );
}

/// Test worker capabilities check is respected by context.
#[tokio::test]
async fn test_e2e_context_capabilities() {
    let (context, _job_store) = make_test_context(
        MockExecutorBehavior::Success {
            exit_code: 0,
            duration: Duration::from_millis(20),
        },
        MockDeliveryBehavior::Success,
    )
    .await;

    let capabilities = context.capabilities();

    assert_eq!(capabilities.max_vcpu, 4);
    assert_eq!(capabilities.max_memory_mb, 4096);
    assert!(capabilities.kernels.contains(&"python:3.12".to_string()));
    assert!(capabilities.kernels.contains(&"node:20".to_string()));
}
