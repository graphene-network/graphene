//! Mock implementation of [`CostCalculator`] for testing.
//!
//! Provides configurable behaviors to test various cost calculation scenarios
//! without requiring real pricing data or complex setup.

use super::calculator::{CostCalculator, ExecutionMetrics};
use super::types::{
    ActualJobCost, CostBreakdown, JobCostEstimate, JobCostSettlement, RefundReason,
};
use super::CostError;
use crate::p2p::messages::{JobManifest, WorkerPricing};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Configurable behavior for the mock cost calculator.
#[derive(Debug, Clone, Default)]
pub enum MockCostBehavior {
    /// Always return a fixed estimate.
    FixedEstimate(JobCostEstimate),

    /// Always return a fixed actual cost.
    FixedActual(ActualJobCost),

    /// Return overflow error.
    Overflow,

    /// Use a simple formula: cost = vcpu * duration_ms (for predictable testing).
    #[default]
    Simple,

    /// Delegate to the default calculator.
    Delegate,
}

/// Mock cost calculator for testing.
///
/// Allows configuring specific behaviors to test edge cases and error handling.
#[derive(Debug)]
pub struct MockCostCalculator {
    estimate_behavior: Mutex<MockCostBehavior>,
    actual_behavior: Mutex<MockCostBehavior>,
    estimate_call_count: AtomicUsize,
    actual_call_count: AtomicUsize,
    settle_call_count: AtomicUsize,
}

impl Default for MockCostCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCostCalculator {
    /// Creates a new mock calculator with default (simple) behavior.
    pub fn new() -> Self {
        Self {
            estimate_behavior: Mutex::new(MockCostBehavior::default()),
            actual_behavior: Mutex::new(MockCostBehavior::default()),
            estimate_call_count: AtomicUsize::new(0),
            actual_call_count: AtomicUsize::new(0),
            settle_call_count: AtomicUsize::new(0),
        }
    }

    /// Creates a mock that always returns overflow errors.
    pub fn overflowing() -> Self {
        Self {
            estimate_behavior: Mutex::new(MockCostBehavior::Overflow),
            actual_behavior: Mutex::new(MockCostBehavior::Overflow),
            estimate_call_count: AtomicUsize::new(0),
            actual_call_count: AtomicUsize::new(0),
            settle_call_count: AtomicUsize::new(0),
        }
    }

    /// Creates a mock that returns fixed estimates.
    pub fn with_fixed_estimate(estimate: JobCostEstimate) -> Self {
        Self {
            estimate_behavior: Mutex::new(MockCostBehavior::FixedEstimate(estimate)),
            actual_behavior: Mutex::new(MockCostBehavior::Simple),
            estimate_call_count: AtomicUsize::new(0),
            actual_call_count: AtomicUsize::new(0),
            settle_call_count: AtomicUsize::new(0),
        }
    }

    /// Set the behavior for estimate calls.
    pub fn set_estimate_behavior(&self, behavior: MockCostBehavior) {
        *self.estimate_behavior.lock().unwrap() = behavior;
    }

    /// Set the behavior for actual cost calls.
    pub fn set_actual_behavior(&self, behavior: MockCostBehavior) {
        *self.actual_behavior.lock().unwrap() = behavior;
    }

    /// Get the number of estimate calls.
    pub fn estimate_call_count(&self) -> usize {
        self.estimate_call_count.load(Ordering::SeqCst)
    }

    /// Get the number of actual cost calls.
    pub fn actual_call_count(&self) -> usize {
        self.actual_call_count.load(Ordering::SeqCst)
    }

    /// Get the number of settle calls.
    pub fn settle_call_count(&self) -> usize {
        self.settle_call_count.load(Ordering::SeqCst)
    }

    /// Reset all call counters.
    pub fn reset_counts(&self) {
        self.estimate_call_count.store(0, Ordering::SeqCst);
        self.actual_call_count.store(0, Ordering::SeqCst);
        self.settle_call_count.store(0, Ordering::SeqCst);
    }
}

impl CostCalculator for MockCostCalculator {
    fn estimate(
        &self,
        manifest: &JobManifest,
        _pricing: &WorkerPricing,
    ) -> Result<JobCostEstimate, CostError> {
        self.estimate_call_count.fetch_add(1, Ordering::SeqCst);

        let behavior = self.estimate_behavior.lock().unwrap().clone();
        match behavior {
            MockCostBehavior::FixedEstimate(est) => Ok(est),
            MockCostBehavior::Overflow => Err(CostError::Overflow),
            MockCostBehavior::Simple => {
                // Simple formula for testing: cost = vcpu * timeout_ms
                let cost = manifest.vcpu as u64 * manifest.timeout_ms;
                Ok(JobCostEstimate::new(cost, 0, 0, 0))
            }
            MockCostBehavior::FixedActual(_) => {
                // Use simple formula if FixedActual is set (it's for actual, not estimate)
                let cost = manifest.vcpu as u64 * manifest.timeout_ms;
                Ok(JobCostEstimate::new(cost, 0, 0, 0))
            }
            MockCostBehavior::Delegate => {
                // Use simple formula as default behavior
                let cost = manifest.vcpu as u64 * manifest.timeout_ms;
                Ok(JobCostEstimate::new(cost, 0, 0, 0))
            }
        }
    }

    fn actual(
        &self,
        metrics: &ExecutionMetrics,
        _pricing: &WorkerPricing,
    ) -> Result<ActualJobCost, CostError> {
        self.actual_call_count.fetch_add(1, Ordering::SeqCst);

        let behavior = self.actual_behavior.lock().unwrap().clone();
        match behavior {
            MockCostBehavior::FixedActual(act) => Ok(act),
            MockCostBehavior::Overflow => Err(CostError::Overflow),
            MockCostBehavior::Simple | MockCostBehavior::Delegate => {
                // Simple formula: cost = vcpu * actual_duration_ms
                let cost = metrics.vcpu as u64 * metrics.actual_duration_ms;
                Ok(ActualJobCost::new(CostBreakdown::new(cost, 0, 0, 0, 0)))
            }
            MockCostBehavior::FixedEstimate(_) => {
                // Use simple formula if FixedEstimate is set (it's for estimate, not actual)
                let cost = metrics.vcpu as u64 * metrics.actual_duration_ms;
                Ok(ActualJobCost::new(CostBreakdown::new(cost, 0, 0, 0, 0)))
            }
        }
    }

    fn settle(
        &self,
        job_id: &str,
        channel_id: [u8; 32],
        max: &JobCostEstimate,
        actual: &ActualJobCost,
        exit_code: i32,
    ) -> JobCostSettlement {
        self.settle_call_count.fetch_add(1, Ordering::SeqCst);

        // Simple settlement logic
        let (final_charge, refund_reason) = match exit_code {
            0..=127 => (actual.total_cost_micros, None),
            200 | 201 => (0, Some(RefundReason::WorkerCrash)),
            202 => (
                actual.total_cost_micros / 2,
                Some(RefundReason::BuildFailure),
            ),
            _ => (actual.total_cost_micros, None),
        };

        let refund = max.max_cost_micros.saturating_sub(final_charge);
        let final_reason = if refund > 0 && refund_reason.is_none() {
            Some(RefundReason::EarlyCompletion)
        } else {
            refund_reason
        };

        JobCostSettlement {
            job_id: job_id.to_string(),
            channel_id,
            max_cost_micros: max.max_cost_micros,
            actual_cost_micros: actual.total_cost_micros,
            final_charge_micros: final_charge,
            refund_micros: refund,
            refund_reason: final_reason,
            exit_code,
            breakdown: actual.breakdown.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 256,
            timeout_ms: 1000,
            runtime: "test".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    fn make_pricing() -> WorkerPricing {
        WorkerPricing::default()
    }

    #[test]
    fn test_mock_simple_estimate() {
        let calc = MockCostCalculator::new();
        let manifest = make_manifest();
        let pricing = make_pricing();

        let estimate = calc.estimate(&manifest, &pricing).unwrap();
        // Simple: 2 vcpu * 1000 ms = 2000
        assert_eq!(estimate.max_cost_micros, 2000);
        assert_eq!(calc.estimate_call_count(), 1);
    }

    #[test]
    fn test_mock_fixed_estimate() {
        let fixed = JobCostEstimate::new(5000, 0, 0, 0);
        let calc = MockCostCalculator::with_fixed_estimate(fixed.clone());

        let estimate = calc.estimate(&make_manifest(), &make_pricing()).unwrap();
        assert_eq!(estimate, fixed);
    }

    #[test]
    fn test_mock_overflow() {
        let calc = MockCostCalculator::overflowing();

        let result = calc.estimate(&make_manifest(), &make_pricing());
        assert!(matches!(result, Err(CostError::Overflow)));

        let metrics = ExecutionMetrics::new(100, 1, 256, 0, 0, 0);
        let result = calc.actual(&metrics, &make_pricing());
        assert!(matches!(result, Err(CostError::Overflow)));
    }

    #[test]
    fn test_mock_call_counts() {
        let calc = MockCostCalculator::new();
        let manifest = make_manifest();
        let pricing = make_pricing();

        let _ = calc.estimate(&manifest, &pricing);
        let _ = calc.estimate(&manifest, &pricing);
        let _ = calc.estimate(&manifest, &pricing);

        assert_eq!(calc.estimate_call_count(), 3);
        assert_eq!(calc.actual_call_count(), 0);

        calc.reset_counts();
        assert_eq!(calc.estimate_call_count(), 0);
    }

    #[test]
    fn test_mock_settle() {
        let calc = MockCostCalculator::new();
        let max = JobCostEstimate::new(1000, 0, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(500, 0, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 0);
        assert_eq!(settlement.final_charge_micros, 500);
        assert_eq!(settlement.refund_micros, 500);
        assert_eq!(calc.settle_call_count(), 1);
    }
}
