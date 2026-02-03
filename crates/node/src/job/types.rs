//! Job types for tracking job lifecycle and computing metrics.

use super::state::{JobState, UserJobState};
use super::JobError;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};

/// Exit code constants for job execution results.
pub mod exit_code {
    /// Success - job completed normally
    pub const SUCCESS: i32 = 0;

    /// User code error range start (1-127)
    pub const USER_ERROR_MIN: i32 = 1;
    /// User code error range end (1-127)
    pub const USER_ERROR_MAX: i32 = 127;

    /// User-configured timeout exceeded
    pub const USER_TIMEOUT: i32 = 128;

    /// Worker crashed during execution
    pub const WORKER_CRASH: i32 = 200;
    /// Worker ran out of resources (memory, disk)
    pub const WORKER_RESOURCE_EXHAUSTED: i32 = 201;
    /// Build failed (Dockerfile/Kraftfile error)
    pub const BUILD_FAILURE: i32 = 202;

    /// Returns true if the exit code indicates a worker fault.
    pub fn is_worker_fault(code: i32) -> bool {
        code == WORKER_CRASH || code == WORKER_RESOURCE_EXHAUSTED
    }

    /// Returns true if the exit code indicates a build failure.
    pub fn is_build_failure(code: i32) -> bool {
        code == BUILD_FAILURE
    }

    /// Returns true if the exit code indicates user fault (code error or timeout).
    pub fn is_user_fault(code: i32) -> bool {
        code == SUCCESS || (USER_ERROR_MIN..=USER_ERROR_MAX).contains(&code) || code == USER_TIMEOUT
    }
}

/// Refund policy computed from job exit code.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RefundPolicy {
    /// Percentage of payment refunded to user (0-100)
    pub user_refund_percent: u8,
    /// Percentage of payment given to worker (0-100)
    pub worker_payment_percent: u8,
}

impl RefundPolicy {
    /// Computes the refund policy from an exit code.
    ///
    /// | Exit Code | Meaning | User Refund | Worker Paid |
    /// |-----------|---------|-------------|-------------|
    /// | 0 | Success | 0% | 100% |
    /// | 1-127 | User code error | 0% | 100% |
    /// | 128 | User timeout | 0% | 100% |
    /// | 200 | Worker crash | 100% | 0% |
    /// | 201 | Worker resource exhausted | 100% | 0% |
    /// | 202 | Build failure | 50% | 50% |
    pub fn from_exit_code(code: i32) -> Self {
        if exit_code::is_worker_fault(code) {
            RefundPolicy {
                user_refund_percent: 100,
                worker_payment_percent: 0,
            }
        } else if exit_code::is_build_failure(code) {
            RefundPolicy {
                user_refund_percent: 50,
                worker_payment_percent: 50,
            }
        } else {
            // Success, user code error, or user timeout - worker gets paid
            RefundPolicy {
                user_refund_percent: 0,
                worker_payment_percent: 100,
            }
        }
    }
}

/// A state transition record with timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransition {
    /// The state transitioned to
    pub state: JobState,
    /// Unix timestamp in milliseconds when the transition occurred
    pub timestamp_ms: u64,
}

impl StateTransition {
    /// Creates a new state transition with the current timestamp.
    pub fn new(state: JobState) -> Self {
        Self {
            state,
            timestamp_ms: current_time_ms(),
        }
    }

    /// Creates a new state transition with a specific timestamp (for testing).
    pub fn with_timestamp(state: JobState, timestamp_ms: u64) -> Self {
        Self {
            state,
            timestamp_ms,
        }
    }
}

/// Metrics computed from job state history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Time spent in queue (Pending → Accepted), in milliseconds
    pub queue_ms: Option<u64>,
    /// Time spent building (Building state), in milliseconds (None if cache hit)
    pub build_ms: Option<u64>,
    /// Time from Accepted to Running (includes build or cache load)
    pub boot_ms: Option<u64>,
    /// Time spent executing (Running → terminal), in milliseconds
    pub execution_ms: Option<u64>,
    /// Total time from Pending to execution complete, in milliseconds
    pub total_ms: Option<u64>,
    /// Whether the job used a cached unikernel
    pub cache_hit: bool,
}

/// A job with its current state and history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier
    pub id: String,
    /// Current job state
    pub state: JobState,
    /// Unix timestamp in milliseconds when the job was created
    pub created_at_ms: u64,
    /// History of state transitions
    pub state_history: Vec<StateTransition>,
    /// Exit code (set when transitioning to Succeeded/Failed/Timeout)
    pub exit_code: Option<i32>,
    /// Hash of the encrypted result blob (set when transitioning to Delivering)
    pub result_hash: Option<Hash>,
    /// ID of the worker processing this job
    pub worker_id: Option<String>,
}

impl Job {
    /// Creates a new job in the Pending state.
    pub fn new(id: impl Into<String>) -> Self {
        let now = current_time_ms();
        Self {
            id: id.into(),
            state: JobState::Pending,
            created_at_ms: now,
            state_history: vec![StateTransition::with_timestamp(JobState::Pending, now)],
            exit_code: None,
            result_hash: None,
            worker_id: None,
        }
    }

    /// Transitions the job to a new state.
    ///
    /// # Errors
    ///
    /// Returns `JobError::InvalidTransition` if the transition is not valid.
    /// Returns `JobError::TerminalState` if the job is already in a terminal state.
    /// Returns `JobError::ExitCodeRequired` if transitioning to a terminal execution
    /// state without providing an exit code (use `transition_with_exit_code` instead).
    pub fn transition(&mut self, new_state: JobState) -> Result<(), JobError> {
        // Check if we're in a terminal state
        if self.state.is_terminal() {
            return Err(JobError::TerminalState(self.state));
        }

        // Check if transition is valid
        if !self.state.can_transition_to(new_state) {
            return Err(JobError::InvalidTransition {
                from: self.state,
                to: new_state,
            });
        }

        // Require exit code for execution-terminating transitions
        if matches!(
            new_state,
            JobState::Succeeded | JobState::Failed | JobState::Timeout
        ) && self.exit_code.is_none()
        {
            return Err(JobError::ExitCodeRequired(new_state));
        }

        self.state = new_state;
        self.state_history.push(StateTransition::new(new_state));
        Ok(())
    }

    /// Transitions the job to a new state with an exit code.
    ///
    /// Use this method when transitioning to Succeeded, Failed, or Timeout states.
    ///
    /// # Errors
    ///
    /// Returns `JobError::InvalidTransition` if the transition is not valid.
    /// Returns `JobError::TerminalState` if the job is already in a terminal state.
    pub fn transition_with_exit_code(
        &mut self,
        new_state: JobState,
        exit_code: i32,
    ) -> Result<(), JobError> {
        self.exit_code = Some(exit_code);
        self.transition(new_state)
    }

    /// Transitions the job to the Delivering state with a result hash.
    ///
    /// # Errors
    ///
    /// Returns `JobError::InvalidTransition` if the current state cannot transition to Delivering.
    pub fn transition_to_delivering(&mut self, result_hash: Hash) -> Result<(), JobError> {
        self.result_hash = Some(result_hash);
        self.transition(JobState::Delivering)
    }

    /// Returns true if the job is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Returns the user-visible state for this job.
    pub fn user_visible_state(&self) -> UserJobState {
        self.state.into()
    }

    /// Returns the refund policy based on the job's exit code.
    ///
    /// Returns `None` if the job has not completed execution (no exit code set).
    pub fn refund_policy(&self) -> Option<RefundPolicy> {
        self.exit_code.map(RefundPolicy::from_exit_code)
    }

    /// Sets the worker ID for this job.
    pub fn set_worker(&mut self, worker_id: impl Into<String>) {
        self.worker_id = Some(worker_id.into());
    }

    /// Computes metrics from the job's state history.
    ///
    /// Metrics are only computed for states that have been reached.
    pub fn compute_metrics(&self) -> JobMetrics {
        let mut metrics = JobMetrics::default();

        // Find timestamps for each relevant state
        let pending_ts = self.find_state_timestamp(JobState::Pending);
        let accepted_ts = self.find_state_timestamp(JobState::Accepted);
        let building_ts = self.find_state_timestamp(JobState::Building);
        let cached_ts = self.find_state_timestamp(JobState::Cached);
        let running_ts = self.find_state_timestamp(JobState::Running);
        let execution_complete_ts = self
            .find_state_timestamp(JobState::Succeeded)
            .or_else(|| self.find_state_timestamp(JobState::Failed))
            .or_else(|| self.find_state_timestamp(JobState::Timeout));

        // Queue time: Pending → Accepted
        if let (Some(p), Some(a)) = (pending_ts, accepted_ts) {
            metrics.queue_ms = Some(a.saturating_sub(p));
        }

        // Determine cache hit
        metrics.cache_hit = cached_ts.is_some();

        // Build time: Building → Running (only if Building state was entered)
        if let (Some(b), Some(r)) = (building_ts, running_ts) {
            metrics.build_ms = Some(r.saturating_sub(b));
        }

        // Boot time: Accepted → Running
        if let (Some(a), Some(r)) = (accepted_ts, running_ts) {
            metrics.boot_ms = Some(r.saturating_sub(a));
        }

        // Execution time: Running → execution complete
        if let (Some(r), Some(e)) = (running_ts, execution_complete_ts) {
            metrics.execution_ms = Some(e.saturating_sub(r));
        }

        // Total time: Pending → execution complete
        if let (Some(p), Some(e)) = (pending_ts, execution_complete_ts) {
            metrics.total_ms = Some(e.saturating_sub(p));
        }

        metrics
    }

    /// Finds the timestamp when a specific state was entered.
    fn find_state_timestamp(&self, state: JobState) -> Option<u64> {
        self.state_history
            .iter()
            .find(|t| t.state == state)
            .map(|t| t.timestamp_ms)
    }
}

/// Returns the current time in milliseconds since Unix epoch.
fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_job() -> Job {
        Job::new("test-job-123")
    }

    #[test]
    fn test_new_job() {
        let job = make_test_job();
        assert_eq!(job.id, "test-job-123");
        assert_eq!(job.state, JobState::Pending);
        assert!(job.exit_code.is_none());
        assert!(job.result_hash.is_none());
        assert!(job.worker_id.is_none());
        assert_eq!(job.state_history.len(), 1);
        assert_eq!(job.state_history[0].state, JobState::Pending);
    }

    #[test]
    fn test_valid_full_lifecycle_cache_miss() {
        let mut job = make_test_job();

        // Pending → Accepted
        assert!(job.transition(JobState::Accepted).is_ok());
        assert_eq!(job.state, JobState::Accepted);

        // Accepted → Building (cache miss)
        assert!(job.transition(JobState::Building).is_ok());
        assert_eq!(job.state, JobState::Building);

        // Building → Running
        assert!(job.transition(JobState::Running).is_ok());
        assert_eq!(job.state, JobState::Running);

        // Running → Succeeded (with exit code)
        assert!(job
            .transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS)
            .is_ok());
        assert_eq!(job.state, JobState::Succeeded);
        assert_eq!(job.exit_code, Some(0));

        // Create a fake hash for testing
        let result_hash = Hash::new(b"test result");

        // Succeeded → Delivering
        assert!(job.transition_to_delivering(result_hash).is_ok());
        assert_eq!(job.state, JobState::Delivering);
        assert_eq!(job.result_hash, Some(result_hash));

        // Delivering → Delivered
        assert!(job.transition(JobState::Delivered).is_ok());
        assert_eq!(job.state, JobState::Delivered);
        assert!(job.is_terminal());

        // Verify full history
        assert_eq!(job.state_history.len(), 7);
    }

    #[test]
    fn test_valid_full_lifecycle_cache_hit() {
        let mut job = make_test_job();

        job.transition(JobState::Accepted).unwrap();
        // Accepted → Cached (cache hit)
        job.transition(JobState::Cached).unwrap();
        assert_eq!(job.state, JobState::Cached);

        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS)
            .unwrap();

        let result_hash = Hash::new(b"test result");
        job.transition_to_delivering(result_hash).unwrap();
        job.transition(JobState::Delivered).unwrap();

        assert!(job.is_terminal());
        assert_eq!(job.state_history.len(), 7);
    }

    #[test]
    fn test_invalid_transition_skip_accepted() {
        let mut job = make_test_job();

        // Pending → Running (invalid: must go through Accepted)
        let err = job.transition(JobState::Running).unwrap_err();
        assert!(matches!(err, JobError::InvalidTransition { .. }));
    }

    #[test]
    fn test_invalid_transition_skip_building_or_cached() {
        let mut job = make_test_job();
        job.transition(JobState::Accepted).unwrap();

        // Accepted → Running (invalid: must go through Building or Cached)
        let err = job.transition(JobState::Running).unwrap_err();
        assert!(matches!(err, JobError::InvalidTransition { .. }));
    }

    #[test]
    fn test_exit_code_required_for_terminal_execution() {
        let mut job = make_test_job();
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();
        job.transition(JobState::Running).unwrap();

        // Running → Succeeded without exit code should fail
        let err = job.transition(JobState::Succeeded).unwrap_err();
        assert!(matches!(err, JobError::ExitCodeRequired(_)));
    }

    #[test]
    fn test_terminal_state_protection() {
        let mut job = make_test_job();
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Cached).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Succeeded, 0)
            .unwrap();
        job.transition_to_delivering(Hash::new(b"test")).unwrap();
        job.transition(JobState::Delivered).unwrap();

        // Any transition from Delivered should fail
        let err = job.transition(JobState::Expired).unwrap_err();
        assert!(matches!(err, JobError::TerminalState(_)));
    }

    #[test]
    fn test_build_failure_from_building_state() {
        let mut job = make_test_job();
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();

        // Building can transition to Failed (build failure)
        job.transition_with_exit_code(JobState::Failed, exit_code::BUILD_FAILURE)
            .unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.exit_code, Some(202));
    }

    #[test]
    fn test_refund_policy_success() {
        let policy = RefundPolicy::from_exit_code(exit_code::SUCCESS);
        assert_eq!(policy.user_refund_percent, 0);
        assert_eq!(policy.worker_payment_percent, 100);
    }

    #[test]
    fn test_refund_policy_user_error() {
        for code in 1..=127 {
            let policy = RefundPolicy::from_exit_code(code);
            assert_eq!(policy.user_refund_percent, 0);
            assert_eq!(policy.worker_payment_percent, 100);
        }
    }

    #[test]
    fn test_refund_policy_user_timeout() {
        let policy = RefundPolicy::from_exit_code(exit_code::USER_TIMEOUT);
        assert_eq!(policy.user_refund_percent, 0);
        assert_eq!(policy.worker_payment_percent, 100);
    }

    #[test]
    fn test_refund_policy_worker_crash() {
        let policy = RefundPolicy::from_exit_code(exit_code::WORKER_CRASH);
        assert_eq!(policy.user_refund_percent, 100);
        assert_eq!(policy.worker_payment_percent, 0);
    }

    #[test]
    fn test_refund_policy_worker_resource_exhausted() {
        let policy = RefundPolicy::from_exit_code(exit_code::WORKER_RESOURCE_EXHAUSTED);
        assert_eq!(policy.user_refund_percent, 100);
        assert_eq!(policy.worker_payment_percent, 0);
    }

    #[test]
    fn test_refund_policy_build_failure() {
        let policy = RefundPolicy::from_exit_code(exit_code::BUILD_FAILURE);
        assert_eq!(policy.user_refund_percent, 50);
        assert_eq!(policy.worker_payment_percent, 50);
    }

    #[test]
    fn test_job_refund_policy() {
        let mut job = make_test_job();
        assert!(job.refund_policy().is_none());

        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Failed, exit_code::WORKER_CRASH)
            .unwrap();

        let policy = job.refund_policy().unwrap();
        assert_eq!(policy.user_refund_percent, 100);
        assert_eq!(policy.worker_payment_percent, 0);
    }

    #[test]
    fn test_user_visible_state() {
        let mut job = make_test_job();
        assert_eq!(job.user_visible_state(), UserJobState::Pending);

        job.transition(JobState::Accepted).unwrap();
        assert_eq!(job.user_visible_state(), UserJobState::Starting);

        job.transition(JobState::Building).unwrap();
        assert_eq!(job.user_visible_state(), UserJobState::Starting);

        job.transition(JobState::Running).unwrap();
        assert_eq!(job.user_visible_state(), UserJobState::Running);

        job.transition_with_exit_code(JobState::Succeeded, 0)
            .unwrap();
        assert_eq!(job.user_visible_state(), UserJobState::Succeeded);
    }

    #[test]
    fn test_set_worker() {
        let mut job = make_test_job();
        assert!(job.worker_id.is_none());

        job.set_worker("worker-456");
        assert_eq!(job.worker_id, Some("worker-456".to_string()));
    }

    #[test]
    fn test_compute_metrics_cache_miss() {
        let mut job = Job {
            id: "test".to_string(),
            state: JobState::Pending,
            created_at_ms: 1000,
            state_history: vec![StateTransition::with_timestamp(JobState::Pending, 1000)],
            exit_code: None,
            result_hash: None,
            worker_id: None,
        };

        // Simulate state transitions with specific timestamps
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Accepted, 1100));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Building, 1200));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Running, 1500));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Succeeded, 2000));
        job.state = JobState::Succeeded;
        job.exit_code = Some(0);

        let metrics = job.compute_metrics();
        assert_eq!(metrics.queue_ms, Some(100)); // 1100 - 1000
        assert_eq!(metrics.build_ms, Some(300)); // 1500 - 1200
        assert_eq!(metrics.boot_ms, Some(400)); // 1500 - 1100
        assert_eq!(metrics.execution_ms, Some(500)); // 2000 - 1500
        assert_eq!(metrics.total_ms, Some(1000)); // 2000 - 1000
        assert!(!metrics.cache_hit);
    }

    #[test]
    fn test_compute_metrics_cache_hit() {
        let mut job = Job {
            id: "test".to_string(),
            state: JobState::Pending,
            created_at_ms: 1000,
            state_history: vec![StateTransition::with_timestamp(JobState::Pending, 1000)],
            exit_code: None,
            result_hash: None,
            worker_id: None,
        };

        // Simulate cache hit path
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Accepted, 1100));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Cached, 1150));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Running, 1200));
        job.state_history
            .push(StateTransition::with_timestamp(JobState::Succeeded, 1700));
        job.state = JobState::Succeeded;
        job.exit_code = Some(0);

        let metrics = job.compute_metrics();
        assert_eq!(metrics.queue_ms, Some(100)); // 1100 - 1000
        assert!(metrics.build_ms.is_none()); // No Building state
        assert_eq!(metrics.boot_ms, Some(100)); // 1200 - 1100
        assert_eq!(metrics.execution_ms, Some(500)); // 1700 - 1200
        assert_eq!(metrics.total_ms, Some(700)); // 1700 - 1000
        assert!(metrics.cache_hit);
    }

    #[test]
    fn test_compute_metrics_partial_completion() {
        let job = Job {
            id: "test".to_string(),
            state: JobState::Building,
            created_at_ms: 1000,
            state_history: vec![
                StateTransition::with_timestamp(JobState::Pending, 1000),
                StateTransition::with_timestamp(JobState::Accepted, 1100),
                StateTransition::with_timestamp(JobState::Building, 1200),
            ],
            exit_code: None,
            result_hash: None,
            worker_id: None,
        };

        let metrics = job.compute_metrics();
        assert_eq!(metrics.queue_ms, Some(100));
        assert!(metrics.build_ms.is_none()); // Not yet completed
        assert!(metrics.boot_ms.is_none()); // Not yet running
        assert!(metrics.execution_ms.is_none());
        assert!(metrics.total_ms.is_none());
        assert!(!metrics.cache_hit);
    }

    #[test]
    fn test_exit_code_helpers() {
        assert!(exit_code::is_worker_fault(exit_code::WORKER_CRASH));
        assert!(exit_code::is_worker_fault(
            exit_code::WORKER_RESOURCE_EXHAUSTED
        ));
        assert!(!exit_code::is_worker_fault(exit_code::BUILD_FAILURE));
        assert!(!exit_code::is_worker_fault(exit_code::SUCCESS));

        assert!(exit_code::is_build_failure(exit_code::BUILD_FAILURE));
        assert!(!exit_code::is_build_failure(exit_code::WORKER_CRASH));

        assert!(exit_code::is_user_fault(exit_code::SUCCESS));
        assert!(exit_code::is_user_fault(1));
        assert!(exit_code::is_user_fault(127));
        assert!(exit_code::is_user_fault(exit_code::USER_TIMEOUT));
        assert!(!exit_code::is_user_fault(exit_code::WORKER_CRASH));
    }

    #[test]
    fn test_serde_job_roundtrip() {
        let mut job = make_test_job();
        job.transition(JobState::Accepted).unwrap();
        job.set_worker("worker-1");

        let json = serde_json::to_string(&job).unwrap();
        let parsed: Job = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, job.id);
        assert_eq!(parsed.state, job.state);
        assert_eq!(parsed.worker_id, job.worker_id);
        assert_eq!(parsed.state_history.len(), job.state_history.len());
    }
}
