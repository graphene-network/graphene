//! Cost tracking for pending jobs to prevent channel over-commitment.
//!
//! This module provides [`CostTracker`] which maintains a record of locked costs
//! for in-flight jobs. This prevents accepting more jobs than a payment channel
//! can cover.
//!
//! # Usage
//!
//! ```ignore
//! // Before accepting a job
//! let max_cost = calculator.estimate(&manifest, &pricing)?;
//! let available = channel_balance - tracker.locked_for_channel(channel_id);
//! if max_cost.max_cost_micros > available {
//!     return Err(RejectReason::InsufficientPayment);
//! }
//! tracker.lock(&job_id, channel_id, max_cost)?;
//!
//! // After job completes
//! tracker.unlock(&job_id);
//! ```

use super::types::JobCostEstimate;
use super::CostError;
use std::collections::HashMap;
use std::sync::RwLock;

/// Record of a locked cost for a pending job.
#[derive(Debug, Clone)]
struct LockedCost {
    /// Payment channel this cost is locked against.
    channel_id: [u8; 32],
    /// Maximum cost locked for this job.
    max_cost: JobCostEstimate,
}

/// Tracks costs locked for pending jobs.
///
/// Thread-safe tracker that maintains locks on payment channels for in-flight jobs.
/// This ensures that the sum of locked costs never exceeds the channel's balance,
/// preventing over-commitment.
///
/// # Thread Safety
///
/// All methods use interior mutability via `RwLock` for concurrent access.
#[derive(Debug, Default)]
pub struct CostTracker {
    /// Map of job_id -> locked cost record.
    locks: RwLock<HashMap<String, LockedCost>>,
}

impl CostTracker {
    /// Creates a new cost tracker.
    pub fn new() -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
        }
    }

    /// Lock a cost amount for a job before execution.
    ///
    /// This should be called after validating that the channel has sufficient
    /// balance (including already-locked amounts) to cover the max cost.
    ///
    /// # Arguments
    /// * `job_id` - Unique job identifier
    /// * `channel_id` - Payment channel to lock against
    /// * `max_cost` - Maximum cost to lock
    ///
    /// # Returns
    /// * `Ok(())` - Cost successfully locked
    /// * `Err(CostError::JobNotFound)` - Should not happen (defensive)
    pub fn lock(
        &self,
        job_id: &str,
        channel_id: [u8; 32],
        max_cost: JobCostEstimate,
    ) -> Result<(), CostError> {
        let mut locks = self.locks.write().unwrap();
        locks.insert(
            job_id.to_string(),
            LockedCost {
                channel_id,
                max_cost,
            },
        );
        Ok(())
    }

    /// Unlock a cost after job completion.
    ///
    /// This releases the locked amount, allowing it to be used for other jobs.
    ///
    /// # Arguments
    /// * `job_id` - The job to unlock
    ///
    /// # Returns
    /// * `Some(JobCostEstimate)` - The cost that was locked (for settlement)
    /// * `None` - Job was not found (already unlocked or never locked)
    pub fn unlock(&self, job_id: &str) -> Option<JobCostEstimate> {
        let mut locks = self.locks.write().unwrap();
        locks.remove(job_id).map(|l| l.max_cost)
    }

    /// Get the total locked amount for a payment channel.
    ///
    /// Use this to check available balance before accepting a new job:
    /// `available = channel_balance - locked_for_channel(channel_id)`
    ///
    /// # Arguments
    /// * `channel_id` - Payment channel to query
    ///
    /// # Returns
    /// Total amount locked across all pending jobs for this channel.
    pub fn locked_for_channel(&self, channel_id: &[u8; 32]) -> u64 {
        let locks = self.locks.read().unwrap();
        locks
            .values()
            .filter(|l| &l.channel_id == channel_id)
            .map(|l| l.max_cost.max_cost_micros)
            .sum()
    }

    /// Get the locked cost for a specific job.
    ///
    /// # Arguments
    /// * `job_id` - Job to query
    ///
    /// # Returns
    /// * `Some((channel_id, max_cost))` - The locked cost and channel
    /// * `None` - Job not found
    pub fn get_lock(&self, job_id: &str) -> Option<([u8; 32], JobCostEstimate)> {
        let locks = self.locks.read().unwrap();
        locks
            .get(job_id)
            .map(|l| (l.channel_id, l.max_cost.clone()))
    }

    /// Check if a job has a locked cost.
    ///
    /// # Arguments
    /// * `job_id` - Job to check
    ///
    /// # Returns
    /// `true` if the job has a locked cost.
    pub fn is_locked(&self, job_id: &str) -> bool {
        let locks = self.locks.read().unwrap();
        locks.contains_key(job_id)
    }

    /// Get the number of locked jobs.
    pub fn lock_count(&self) -> usize {
        let locks = self.locks.read().unwrap();
        locks.len()
    }

    /// Get the number of locked jobs for a specific channel.
    pub fn lock_count_for_channel(&self, channel_id: &[u8; 32]) -> usize {
        let locks = self.locks.read().unwrap();
        locks
            .values()
            .filter(|l| &l.channel_id == channel_id)
            .count()
    }

    /// Clear all locks (for testing or emergency reset).
    #[cfg(test)]
    pub fn clear(&self) {
        let mut locks = self.locks.write().unwrap();
        locks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_estimate(cost: u64) -> JobCostEstimate {
        JobCostEstimate::new(cost, 0, 0, 0)
    }

    #[test]
    fn test_lock_and_unlock() {
        let tracker = CostTracker::new();
        let channel_id = [1u8; 32];
        let estimate = make_estimate(1000);

        // Lock
        tracker.lock("job-1", channel_id, estimate.clone()).unwrap();
        assert!(tracker.is_locked("job-1"));
        assert_eq!(tracker.lock_count(), 1);

        // Unlock
        let unlocked = tracker.unlock("job-1");
        assert!(unlocked.is_some());
        assert_eq!(unlocked.unwrap().max_cost_micros, 1000);
        assert!(!tracker.is_locked("job-1"));
        assert_eq!(tracker.lock_count(), 0);
    }

    #[test]
    fn test_locked_for_channel() {
        let tracker = CostTracker::new();
        let channel_a = [1u8; 32];
        let channel_b = [2u8; 32];

        // Lock jobs on channel A
        tracker
            .lock("job-1", channel_a, make_estimate(1000))
            .unwrap();
        tracker
            .lock("job-2", channel_a, make_estimate(2000))
            .unwrap();

        // Lock job on channel B
        tracker
            .lock("job-3", channel_b, make_estimate(500))
            .unwrap();

        // Check totals
        assert_eq!(tracker.locked_for_channel(&channel_a), 3000);
        assert_eq!(tracker.locked_for_channel(&channel_b), 500);

        // Check counts
        assert_eq!(tracker.lock_count_for_channel(&channel_a), 2);
        assert_eq!(tracker.lock_count_for_channel(&channel_b), 1);
    }

    #[test]
    fn test_get_lock() {
        let tracker = CostTracker::new();
        let channel_id = [1u8; 32];
        let estimate = make_estimate(1000);

        tracker.lock("job-1", channel_id, estimate.clone()).unwrap();

        let lock = tracker.get_lock("job-1");
        assert!(lock.is_some());
        let (ch, cost) = lock.unwrap();
        assert_eq!(ch, channel_id);
        assert_eq!(cost.max_cost_micros, 1000);

        // Non-existent job
        assert!(tracker.get_lock("job-999").is_none());
    }

    #[test]
    fn test_unlock_nonexistent() {
        let tracker = CostTracker::new();
        let unlocked = tracker.unlock("nonexistent");
        assert!(unlocked.is_none());
    }

    #[test]
    fn test_double_lock_overwrites() {
        let tracker = CostTracker::new();
        let channel_id = [1u8; 32];

        tracker
            .lock("job-1", channel_id, make_estimate(1000))
            .unwrap();
        tracker
            .lock("job-1", channel_id, make_estimate(2000))
            .unwrap();

        // Second lock overwrites first
        assert_eq!(tracker.locked_for_channel(&channel_id), 2000);
        assert_eq!(tracker.lock_count(), 1);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let tracker = Arc::new(CostTracker::new());
        let channel_id = [1u8; 32];

        // Spawn multiple threads to lock/unlock
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let tracker = Arc::clone(&tracker);
                thread::spawn(move || {
                    let job_id = format!("job-{}", i);
                    tracker
                        .lock(&job_id, channel_id, make_estimate(100))
                        .unwrap();
                    thread::sleep(std::time::Duration::from_millis(1));
                    tracker.unlock(&job_id);
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // All locks should be released
        assert_eq!(tracker.lock_count(), 0);
        assert_eq!(tracker.locked_for_channel(&channel_id), 0);
    }

    #[test]
    fn test_clear() {
        let tracker = CostTracker::new();
        let channel_id = [1u8; 32];

        tracker
            .lock("job-1", channel_id, make_estimate(1000))
            .unwrap();
        tracker
            .lock("job-2", channel_id, make_estimate(2000))
            .unwrap();

        tracker.clear();

        assert_eq!(tracker.lock_count(), 0);
        assert_eq!(tracker.locked_for_channel(&channel_id), 0);
    }
}
