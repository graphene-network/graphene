//! Job state machine for tracking job lifecycle.
//!
//! This module implements the 11-state job lifecycle per Whitepaper Section 5.7:
//!
//! ```text
//! PENDING → ACCEPTED → [BUILDING|CACHED] → RUNNING → [SUCCEEDED|FAILED|TIMEOUT]
//!                                                            ↓
//!                                                       DELIVERING
//!                                                            ↓
//!                                                   [DELIVERED|EXPIRED]
//! ```
//!
//! # Example
//!
//! ```
//! use graphene_node::job::{Job, JobState, exit_code};
//!
//! let mut job = Job::new("job-123");
//!
//! // Worker accepts the job
//! job.transition(JobState::Accepted).unwrap();
//!
//! // Cache miss - need to build
//! job.transition(JobState::Building).unwrap();
//!
//! // Build complete, start execution
//! job.transition(JobState::Running).unwrap();
//!
//! // Execution succeeds
//! job.transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS).unwrap();
//!
//! // Compute refund policy (worker gets 100% for success)
//! let policy = job.refund_policy().unwrap();
//! assert_eq!(policy.worker_payment_percent, 100);
//! ```

pub mod state;
pub mod types;

pub use state::{JobState, UserJobState};
pub use types::{exit_code, Job, JobMetrics, RefundPolicy, StateTransition};

use thiserror::Error;

/// Errors that can occur during job operations.
#[derive(Debug, Error)]
pub enum JobError {
    /// Invalid state transition attempted.
    #[error("invalid transition from {from} to {to}")]
    InvalidTransition { from: JobState, to: JobState },

    /// Job not found.
    #[error("job not found: {0}")]
    NotFound(String),

    /// Job already exists.
    #[error("job already exists: {0}")]
    AlreadyExists(String),

    /// Operation attempted on a job in a terminal state.
    #[error("job is in terminal state: {0}")]
    TerminalState(JobState),

    /// Exit code required for this transition.
    #[error("exit code required for transition to {0}")]
    ExitCodeRequired(JobState),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hash(data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }

    #[test]
    fn test_full_happy_path_cache_miss() {
        let mut job = Job::new("test-job");
        job.set_worker("worker-1");

        // Full lifecycle: cache miss path
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS)
            .unwrap();

        let result_hash = test_hash(b"encrypted result");
        job.transition_to_delivering(result_hash).unwrap();
        job.transition(JobState::Delivered).unwrap();

        assert!(job.is_terminal());
        assert_eq!(job.user_visible_state(), UserJobState::Delivered);

        let metrics = job.compute_metrics();
        assert!(!metrics.cache_hit);
        assert!(metrics.build_ms.is_some());

        let policy = job.refund_policy().unwrap();
        assert_eq!(policy.worker_payment_percent, 100);
    }

    #[test]
    fn test_full_happy_path_cache_hit() {
        let mut job = Job::new("test-job");

        // Full lifecycle: cache hit path
        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Cached).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS)
            .unwrap();

        let result_hash = test_hash(b"encrypted result");
        job.transition_to_delivering(result_hash).unwrap();
        job.transition(JobState::Delivered).unwrap();

        assert!(job.is_terminal());

        let metrics = job.compute_metrics();
        assert!(metrics.cache_hit);
        assert!(metrics.build_ms.is_none());
    }

    #[test]
    fn test_worker_crash_refund() {
        let mut job = Job::new("test-job");

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
    fn test_build_failure_refund() {
        let mut job = Job::new("test-job");

        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Building).unwrap();
        job.transition_with_exit_code(JobState::Failed, exit_code::BUILD_FAILURE)
            .unwrap();

        let policy = job.refund_policy().unwrap();
        assert_eq!(policy.user_refund_percent, 50);
        assert_eq!(policy.worker_payment_percent, 50);
    }

    #[test]
    fn test_user_timeout_no_refund() {
        let mut job = Job::new("test-job");

        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Cached).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Timeout, exit_code::USER_TIMEOUT)
            .unwrap();

        let policy = job.refund_policy().unwrap();
        assert_eq!(policy.user_refund_percent, 0);
        assert_eq!(policy.worker_payment_percent, 100);
    }

    #[test]
    fn test_delivery_expiration() {
        let mut job = Job::new("test-job");

        job.transition(JobState::Accepted).unwrap();
        job.transition(JobState::Cached).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition_with_exit_code(JobState::Succeeded, 0)
            .unwrap();

        let result_hash = test_hash(b"result");
        job.transition_to_delivering(result_hash).unwrap();

        // Delivery times out
        job.transition(JobState::Expired).unwrap();

        assert!(job.is_terminal());
        assert_eq!(job.user_visible_state(), UserJobState::Expired);
    }

    #[test]
    fn test_error_display() {
        let err = JobError::InvalidTransition {
            from: JobState::Pending,
            to: JobState::Running,
        };
        assert_eq!(
            err.to_string(),
            "invalid transition from pending to running"
        );

        let err = JobError::NotFound("job-123".to_string());
        assert_eq!(err.to_string(), "job not found: job-123");

        let err = JobError::TerminalState(JobState::Delivered);
        assert_eq!(err.to_string(), "job is in terminal state: delivered");

        let err = JobError::ExitCodeRequired(JobState::Succeeded);
        assert_eq!(
            err.to_string(),
            "exit code required for transition to succeeded"
        );
    }
}
