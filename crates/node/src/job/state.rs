//! Job state definitions for the job lifecycle state machine.
//!
//! This module defines two state enums:
//! - `JobState`: Internal 11-state representation used by the system
//! - `UserJobState`: External 9-state representation exposed to users

use serde::{Deserialize, Serialize};

/// Internal job state representing the full lifecycle.
///
/// State transitions follow this graph:
/// ```text
/// PENDING → ACCEPTED → [BUILDING|CACHED] → RUNNING → [SUCCEEDED|FAILED|TIMEOUT]
///                                                            ↓
///                                                       DELIVERING
///                                                            ↓
///                                                   [DELIVERED|EXPIRED]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Job submitted, awaiting worker acceptance
    Pending,
    /// Worker accepted the job, determining if build is needed
    Accepted,
    /// Building unikernel (cache miss)
    Building,
    /// Using cached unikernel (cache hit)
    Cached,
    /// Unikernel execution in progress
    Running,
    /// Execution completed successfully (exit code 0)
    Succeeded,
    /// Execution failed (exit code 1-127 or 200-202)
    Failed,
    /// Execution exceeded time limit (exit code 128)
    Timeout,
    /// Delivering result to requester via P2P
    Delivering,
    /// Result successfully delivered
    Delivered,
    /// Result delivery failed or timed out
    Expired,
}

impl JobState {
    /// Returns the state name as a string slice for metrics labels.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Accepted => "accepted",
            JobState::Building => "building",
            JobState::Cached => "cached",
            JobState::Running => "running",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Timeout => "timeout",
            JobState::Delivering => "delivering",
            JobState::Delivered => "delivered",
            JobState::Expired => "expired",
        }
    }

    /// Returns true if this is a terminal state (no further transitions possible).
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Delivered | JobState::Expired)
    }

    /// Returns true if job execution has completed (success, failure, or timeout).
    ///
    /// This is useful for knowing when execution metrics can be computed,
    /// even though the job may still need to deliver results.
    pub fn is_execution_complete(&self) -> bool {
        matches!(
            self,
            JobState::Succeeded
                | JobState::Failed
                | JobState::Timeout
                | JobState::Delivering
                | JobState::Delivered
                | JobState::Expired
        )
    }

    /// Returns the valid next states for this state.
    pub fn valid_transitions(&self) -> &'static [JobState] {
        match self {
            JobState::Pending => &[JobState::Accepted],
            JobState::Accepted => &[JobState::Building, JobState::Cached],
            JobState::Building => &[JobState::Running, JobState::Failed],
            JobState::Cached => &[JobState::Running],
            JobState::Running => &[JobState::Succeeded, JobState::Failed, JobState::Timeout],
            JobState::Succeeded | JobState::Failed | JobState::Timeout => &[JobState::Delivering],
            JobState::Delivering => &[JobState::Delivered, JobState::Expired],
            JobState::Delivered | JobState::Expired => &[],
        }
    }

    /// Returns true if transitioning to the given state is valid.
    pub fn can_transition_to(&self, next: JobState) -> bool {
        self.valid_transitions().contains(&next)
    }
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// User-visible job state with simplified representation.
///
/// This collapses internal states (Building/Cached) into user-friendly states
/// and renames execution-complete states to indicate result availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserJobState {
    /// Job submitted, awaiting processing
    Pending,
    /// Job is starting (building or loading from cache)
    Starting,
    /// Job is running
    Running,
    /// Job succeeded
    Succeeded,
    /// Job failed
    Failed,
    /// Job timed out
    Timeout,
    /// Result is ready for download
    Ready,
    /// Result has been delivered
    Delivered,
    /// Result expired before delivery
    Expired,
}

impl UserJobState {
    /// Returns the state name as a string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            UserJobState::Pending => "pending",
            UserJobState::Starting => "starting",
            UserJobState::Running => "running",
            UserJobState::Succeeded => "succeeded",
            UserJobState::Failed => "failed",
            UserJobState::Timeout => "timeout",
            UserJobState::Ready => "ready",
            UserJobState::Delivered => "delivered",
            UserJobState::Expired => "expired",
        }
    }
}

impl std::fmt::Display for UserJobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<JobState> for UserJobState {
    fn from(state: JobState) -> Self {
        match state {
            JobState::Pending => UserJobState::Pending,
            JobState::Accepted | JobState::Building | JobState::Cached => UserJobState::Starting,
            JobState::Running => UserJobState::Running,
            JobState::Succeeded => UserJobState::Succeeded,
            JobState::Failed => UserJobState::Failed,
            JobState::Timeout => UserJobState::Timeout,
            JobState::Delivering => UserJobState::Ready,
            JobState::Delivered => UserJobState::Delivered,
            JobState::Expired => UserJobState::Expired,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_state_as_str() {
        assert_eq!(JobState::Pending.as_str(), "pending");
        assert_eq!(JobState::Building.as_str(), "building");
        assert_eq!(JobState::Delivered.as_str(), "delivered");
    }

    #[test]
    fn test_job_state_display() {
        assert_eq!(format!("{}", JobState::Running), "running");
        assert_eq!(format!("{}", JobState::Timeout), "timeout");
    }

    #[test]
    fn test_terminal_states() {
        assert!(!JobState::Pending.is_terminal());
        assert!(!JobState::Running.is_terminal());
        assert!(!JobState::Succeeded.is_terminal());
        assert!(!JobState::Delivering.is_terminal());
        assert!(JobState::Delivered.is_terminal());
        assert!(JobState::Expired.is_terminal());
    }

    #[test]
    fn test_execution_complete_states() {
        assert!(!JobState::Pending.is_execution_complete());
        assert!(!JobState::Building.is_execution_complete());
        assert!(!JobState::Running.is_execution_complete());
        assert!(JobState::Succeeded.is_execution_complete());
        assert!(JobState::Failed.is_execution_complete());
        assert!(JobState::Timeout.is_execution_complete());
        assert!(JobState::Delivering.is_execution_complete());
        assert!(JobState::Delivered.is_execution_complete());
    }

    #[test]
    fn test_valid_transitions_pending() {
        assert!(JobState::Pending.can_transition_to(JobState::Accepted));
        assert!(!JobState::Pending.can_transition_to(JobState::Running));
        assert!(!JobState::Pending.can_transition_to(JobState::Building));
    }

    #[test]
    fn test_valid_transitions_accepted() {
        assert!(JobState::Accepted.can_transition_to(JobState::Building));
        assert!(JobState::Accepted.can_transition_to(JobState::Cached));
        assert!(!JobState::Accepted.can_transition_to(JobState::Running));
    }

    #[test]
    fn test_valid_transitions_building() {
        assert!(JobState::Building.can_transition_to(JobState::Running));
        assert!(JobState::Building.can_transition_to(JobState::Failed));
        assert!(!JobState::Building.can_transition_to(JobState::Succeeded));
    }

    #[test]
    fn test_valid_transitions_running() {
        assert!(JobState::Running.can_transition_to(JobState::Succeeded));
        assert!(JobState::Running.can_transition_to(JobState::Failed));
        assert!(JobState::Running.can_transition_to(JobState::Timeout));
        assert!(!JobState::Running.can_transition_to(JobState::Delivered));
    }

    #[test]
    fn test_valid_transitions_terminal() {
        assert!(!JobState::Delivered.can_transition_to(JobState::Expired));
        assert!(!JobState::Expired.can_transition_to(JobState::Delivered));
        assert!(JobState::Delivered.valid_transitions().is_empty());
        assert!(JobState::Expired.valid_transitions().is_empty());
    }

    #[test]
    fn test_user_state_from_job_state() {
        assert_eq!(UserJobState::from(JobState::Pending), UserJobState::Pending);
        assert_eq!(
            UserJobState::from(JobState::Accepted),
            UserJobState::Starting
        );
        assert_eq!(
            UserJobState::from(JobState::Building),
            UserJobState::Starting
        );
        assert_eq!(UserJobState::from(JobState::Cached), UserJobState::Starting);
        assert_eq!(UserJobState::from(JobState::Running), UserJobState::Running);
        assert_eq!(
            UserJobState::from(JobState::Succeeded),
            UserJobState::Succeeded
        );
        assert_eq!(UserJobState::from(JobState::Failed), UserJobState::Failed);
        assert_eq!(UserJobState::from(JobState::Timeout), UserJobState::Timeout);
        assert_eq!(
            UserJobState::from(JobState::Delivering),
            UserJobState::Ready
        );
        assert_eq!(
            UserJobState::from(JobState::Delivered),
            UserJobState::Delivered
        );
        assert_eq!(UserJobState::from(JobState::Expired), UserJobState::Expired);
    }

    #[test]
    fn test_user_state_as_str() {
        assert_eq!(UserJobState::Starting.as_str(), "starting");
        assert_eq!(UserJobState::Ready.as_str(), "ready");
    }

    #[test]
    fn test_serde_roundtrip() {
        let state = JobState::Running;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"running\"");
        let parsed: JobState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }
}
