//! Cost calculation trait and implementation.
//!
//! This module provides the [`CostCalculator`] trait for computing job costs
//! based on resource usage and worker pricing.

use super::types::{
    ActualJobCost, CostBreakdown, JobCostEstimate, JobCostSettlement, RefundReason,
};
use super::CostError;
use crate::p2p::messages::{JobManifest, WorkerPricing};

/// Metrics captured during job execution.
///
/// These are used to calculate the actual cost after execution completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionMetrics {
    /// Actual execution duration in milliseconds.
    pub actual_duration_ms: u64,

    /// Number of vCPUs allocated.
    pub vcpu: u8,

    /// Memory allocated in MB.
    pub memory_mb: u32,

    /// Network egress in bytes (VM -> external).
    pub egress_bytes: u64,

    /// Network ingress in bytes (external -> VM).
    pub ingress_bytes: u64,

    /// Exit code of the job.
    pub exit_code: i32,
}

impl ExecutionMetrics {
    /// Creates new execution metrics.
    pub fn new(
        actual_duration_ms: u64,
        vcpu: u8,
        memory_mb: u32,
        egress_bytes: u64,
        ingress_bytes: u64,
        exit_code: i32,
    ) -> Self {
        Self {
            actual_duration_ms,
            vcpu,
            memory_mb,
            egress_bytes,
            ingress_bytes,
            exit_code,
        }
    }
}

/// Trait for calculating job costs.
///
/// Implementations provide methods to:
/// - Estimate maximum cost before execution
/// - Calculate actual cost after execution
/// - Settle final charges with refund policy
pub trait CostCalculator: Send + Sync {
    /// Estimate the maximum cost for a job before execution.
    ///
    /// This uses the manifest's timeout and resource requirements to compute
    /// the worst-case cost. The user's payment ticket must authorize at least
    /// this amount.
    ///
    /// # Arguments
    /// * `manifest` - Job resource requirements and timeout
    /// * `pricing` - Worker's pricing for resources
    ///
    /// # Returns
    /// * `Ok(JobCostEstimate)` - The maximum possible cost
    /// * `Err(CostError::Overflow)` - If calculation would overflow
    fn estimate(
        &self,
        manifest: &JobManifest,
        pricing: &WorkerPricing,
    ) -> Result<JobCostEstimate, CostError>;

    /// Calculate the actual cost based on execution metrics.
    ///
    /// This uses the real resource usage to compute the actual cost.
    /// Will always be <= estimate for the same job (since actual_duration <= timeout).
    ///
    /// # Arguments
    /// * `metrics` - Actual resource usage from execution
    /// * `pricing` - Worker's pricing for resources
    ///
    /// # Returns
    /// * `Ok(ActualJobCost)` - The actual cost incurred
    /// * `Err(CostError::Overflow)` - If calculation would overflow
    fn actual(
        &self,
        metrics: &ExecutionMetrics,
        pricing: &WorkerPricing,
    ) -> Result<ActualJobCost, CostError>;

    /// Settle the job cost and calculate refund.
    ///
    /// Applies the refund policy based on exit code:
    /// - Exit 0, 1-127: Charge 100% of actual cost (success or user error)
    /// - Exit 200, 201: Charge 0% (worker crash - full refund)
    /// - Exit 202: Charge 50% of actual cost (build failure)
    /// - Timeout: Charge max_cost (user authorized full duration)
    ///
    /// # Arguments
    /// * `job_id` - Unique job identifier
    /// * `channel_id` - Payment channel for this job
    /// * `max` - The estimated maximum cost (locked amount)
    /// * `actual` - The actual cost from execution
    /// * `exit_code` - Job exit code (determines refund policy)
    ///
    /// # Returns
    /// Settlement record with final charge and refund amounts
    fn settle(
        &self,
        job_id: &str,
        channel_id: [u8; 32],
        max: &JobCostEstimate,
        actual: &ActualJobCost,
        exit_code: i32,
    ) -> JobCostSettlement;
}

/// Default implementation of [`CostCalculator`].
///
/// Uses the whitepaper formula for cost calculation:
/// - CPU cost = vcpu × duration_ms × cpu_ms_micros
/// - Memory cost = memory_mb × duration_ms × memory_mb_ms_micros
/// - Egress cost = egress_bytes / (1024 * 1024) × egress_mb_micros (Phase 2)
#[derive(Debug, Clone, Default)]
pub struct DefaultCostCalculator;

impl DefaultCostCalculator {
    /// Creates a new cost calculator.
    pub fn new() -> Self {
        Self
    }

    /// Calculate CPU cost: vcpu * duration_ms * cpu_ms_micros
    fn calculate_cpu_cost(
        &self,
        vcpu: u8,
        duration_ms: u64,
        cpu_ms_micros: u64,
    ) -> Result<u64, CostError> {
        (vcpu as u64)
            .checked_mul(duration_ms)
            .and_then(|v| v.checked_mul(cpu_ms_micros))
            .ok_or(CostError::Overflow)
    }

    /// Calculate memory cost: memory_mb * duration_ms * memory_mb_ms_micros
    fn calculate_memory_cost(
        &self,
        memory_mb: u32,
        duration_ms: u64,
        memory_mb_ms_micros: f64,
    ) -> Result<u64, CostError> {
        let cost = (memory_mb as f64) * (duration_ms as f64) * memory_mb_ms_micros;
        if cost.is_infinite() || cost.is_nan() || cost < 0.0 || cost > u64::MAX as f64 {
            return Err(CostError::Overflow);
        }
        Ok(cost.round() as u64)
    }

    /// Calculate egress cost: egress_bytes / MB * egress_mb_micros
    fn calculate_egress_cost(
        &self,
        egress_bytes: u64,
        egress_mb_micros: Option<f64>,
    ) -> Result<u64, CostError> {
        match egress_mb_micros {
            Some(price) => {
                let egress_mb = egress_bytes as f64 / (1024.0 * 1024.0);
                let cost = egress_mb * price;
                if cost.is_infinite() || cost.is_nan() || cost < 0.0 || cost > u64::MAX as f64 {
                    return Err(CostError::Overflow);
                }
                Ok(cost.round() as u64)
            }
            None => Ok(0),
        }
    }

    /// Calculate ingress cost: ingress_bytes / MB * ingress_mb_micros
    fn calculate_ingress_cost(
        &self,
        ingress_bytes: u64,
        ingress_mb_micros: Option<f64>,
    ) -> Result<u64, CostError> {
        match ingress_mb_micros {
            Some(price) => {
                let ingress_mb = ingress_bytes as f64 / (1024.0 * 1024.0);
                let cost = ingress_mb * price;
                if cost.is_infinite() || cost.is_nan() || cost < 0.0 || cost > u64::MAX as f64 {
                    return Err(CostError::Overflow);
                }
                Ok(cost.round() as u64)
            }
            None => Ok(0),
        }
    }

    /// Determine the charge percentage based on exit code.
    ///
    /// Returns (charge_percentage, refund_reason):
    /// - (100, None) for success or user error
    /// - (0, Some(WorkerCrash)) for worker crash
    /// - (50, Some(BuildFailure)) for build failure
    fn get_charge_policy(&self, exit_code: i32) -> (u8, Option<RefundReason>) {
        match exit_code {
            // Success or user code error - full charge
            0..=127 => (100, None),

            // Worker crash (exit 200) or kernel panic (exit 201) - full refund
            200 | 201 => (0, Some(RefundReason::WorkerCrash)),

            // Build failure (exit 202) - 50% charge
            202 => (50, Some(RefundReason::BuildFailure)),

            // Timeout is indicated by exit -1 (set by VmmRunner)
            -1 => (100, None), // Timeout: charge full max_cost (handled specially)

            // Unknown exit codes - charge full actual cost
            _ => (100, None),
        }
    }
}

impl CostCalculator for DefaultCostCalculator {
    fn estimate(
        &self,
        manifest: &JobManifest,
        pricing: &WorkerPricing,
    ) -> Result<JobCostEstimate, CostError> {
        let cpu_cost =
            self.calculate_cpu_cost(manifest.vcpu, manifest.timeout_ms, pricing.cpu_ms_micros)?;

        let memory_cost = self.calculate_memory_cost(
            manifest.memory_mb,
            manifest.timeout_ms,
            pricing.memory_mb_ms_micros,
        )?;

        // Phase 2: Egress estimation would use manifest.estimated_egress_mb
        // For now, egress cost is 0
        let egress_cost = 0u64;

        // GPU cost (future)
        let gpu_cost = 0u64;

        Ok(JobCostEstimate::new(
            cpu_cost,
            memory_cost,
            egress_cost,
            gpu_cost,
        ))
    }

    fn actual(
        &self,
        metrics: &ExecutionMetrics,
        pricing: &WorkerPricing,
    ) -> Result<ActualJobCost, CostError> {
        let cpu_cost = self.calculate_cpu_cost(
            metrics.vcpu,
            metrics.actual_duration_ms,
            pricing.cpu_ms_micros,
        )?;

        let memory_cost = self.calculate_memory_cost(
            metrics.memory_mb,
            metrics.actual_duration_ms,
            pricing.memory_mb_ms_micros,
        )?;

        // Network metering: calculate egress and ingress costs
        let egress_cost =
            self.calculate_egress_cost(metrics.egress_bytes, pricing.egress_mb_micros)?;
        let ingress_cost =
            self.calculate_ingress_cost(metrics.ingress_bytes, pricing.ingress_mb_micros)?;

        // GPU cost (future)
        let gpu_cost = 0u64;

        let breakdown =
            CostBreakdown::new(cpu_cost, memory_cost, egress_cost, ingress_cost, gpu_cost);

        Ok(ActualJobCost::new(breakdown))
    }

    fn settle(
        &self,
        job_id: &str,
        channel_id: [u8; 32],
        max: &JobCostEstimate,
        actual: &ActualJobCost,
        exit_code: i32,
    ) -> JobCostSettlement {
        let (charge_percentage, refund_reason) = self.get_charge_policy(exit_code);

        // Calculate final charge based on policy
        let final_charge_micros = if exit_code == -1 {
            // Timeout: charge max_cost (user authorized full duration)
            max.max_cost_micros
        } else {
            (actual.total_cost_micros as u128 * charge_percentage as u128 / 100) as u64
        };

        // Refund is the difference between locked amount and final charge
        let refund_micros = max.max_cost_micros.saturating_sub(final_charge_micros);

        // Determine the refund reason
        let final_refund_reason = if refund_micros > 0 && refund_reason.is_none() {
            // There's a refund but no explicit reason (early completion)
            Some(RefundReason::EarlyCompletion)
        } else if refund_micros > 0 {
            refund_reason
        } else {
            None
        };

        JobCostSettlement {
            job_id: job_id.to_string(),
            channel_id,
            max_cost_micros: max.max_cost_micros,
            actual_cost_micros: actual.total_cost_micros,
            final_charge_micros,
            refund_micros,
            refund_reason: final_refund_reason,
            exit_code,
            breakdown: actual.breakdown.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_manifest(vcpu: u8, memory_mb: u32, timeout_ms: u64) -> JobManifest {
        JobManifest {
            vcpu,
            memory_mb,
            timeout_ms,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    fn make_pricing(cpu_ms_micros: u64, memory_mb_ms_micros: f64) -> WorkerPricing {
        WorkerPricing {
            cpu_ms_micros,
            memory_mb_ms_micros,
            disk_gb_ms_micros: None,
            gpu_ms_micros: None,
            egress_mb_micros: None,
            ingress_mb_micros: None,
        }
    }

    #[test]
    fn test_estimate_simple() {
        let calc = DefaultCostCalculator::new();
        let manifest = make_manifest(1, 100, 1000);
        let pricing = make_pricing(1, 1.0);

        let estimate = calc.estimate(&manifest, &pricing).unwrap();

        // CPU: 1 * 1000 * 1 = 1000
        // Memory: 100 * 1000 * 1.0 = 100000
        assert_eq!(estimate.cpu_cost_micros, 1000);
        assert_eq!(estimate.memory_cost_micros, 100000);
        assert_eq!(estimate.max_cost_micros, 101000);
    }

    #[test]
    fn test_estimate_multi_vcpu() {
        let calc = DefaultCostCalculator::new();
        let manifest = make_manifest(4, 512, 10_000);
        let pricing = make_pricing(10, 0.1);

        let estimate = calc.estimate(&manifest, &pricing).unwrap();

        // CPU: 4 * 10000 * 10 = 400000
        // Memory: 512 * 10000 * 0.1 = 512000
        assert_eq!(estimate.cpu_cost_micros, 400_000);
        assert_eq!(estimate.memory_cost_micros, 512_000);
        assert_eq!(estimate.max_cost_micros, 912_000);
    }

    #[test]
    fn test_actual_cost() {
        let calc = DefaultCostCalculator::new();
        let pricing = make_pricing(10, 0.5);
        let metrics = ExecutionMetrics::new(5_000, 2, 256, 0, 0, 0);

        let actual = calc.actual(&metrics, &pricing).unwrap();

        // CPU: 2 * 5000 * 10 = 100000
        // Memory: 256 * 5000 * 0.5 = 640000
        assert_eq!(actual.breakdown.cpu_cost_micros, 100_000);
        assert_eq!(actual.breakdown.memory_cost_micros, 640_000);
        assert_eq!(actual.total_cost_micros, 740_000);
    }

    #[test]
    fn test_settle_success() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(1000, 1000, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(500, 500, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 0);

        assert_eq!(settlement.final_charge_micros, 1000);
        assert_eq!(settlement.refund_micros, 1000); // max 2000 - charge 1000
        assert_eq!(
            settlement.refund_reason,
            Some(RefundReason::EarlyCompletion)
        );
    }

    #[test]
    fn test_settle_user_error() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(500, 500, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(400, 400, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 1);

        // Exit 1 = user code error, charge 100%
        assert_eq!(settlement.final_charge_micros, 800);
        assert_eq!(settlement.refund_micros, 200);
    }

    #[test]
    fn test_settle_worker_crash() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(500, 500, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(200, 200, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 200);

        // Exit 200 = worker crash, charge 0%
        assert_eq!(settlement.final_charge_micros, 0);
        assert_eq!(settlement.refund_micros, 1000);
        assert_eq!(settlement.refund_reason, Some(RefundReason::WorkerCrash));
    }

    #[test]
    fn test_settle_kernel_panic() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(500, 500, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(100, 100, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 201);

        // Exit 201 = kernel panic, charge 0%
        assert_eq!(settlement.final_charge_micros, 0);
        assert_eq!(settlement.refund_micros, 1000);
        assert_eq!(settlement.refund_reason, Some(RefundReason::WorkerCrash));
    }

    #[test]
    fn test_settle_build_failure() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(500, 500, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(300, 300, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, 202);

        // Exit 202 = build failure, charge 50%
        assert_eq!(settlement.final_charge_micros, 300); // 50% of 600
        assert_eq!(settlement.refund_micros, 700);
        assert_eq!(settlement.refund_reason, Some(RefundReason::BuildFailure));
    }

    #[test]
    fn test_settle_timeout() {
        let calc = DefaultCostCalculator::new();
        let max = JobCostEstimate::new(500, 500, 0, 0);
        let actual = ActualJobCost::new(CostBreakdown::new(500, 500, 0, 0, 0));

        let settlement = calc.settle("job-1", [0u8; 32], &max, &actual, -1);

        // Timeout = charge max_cost
        assert_eq!(settlement.final_charge_micros, 1000);
        assert_eq!(settlement.refund_micros, 0);
        assert_eq!(settlement.refund_reason, None);
    }

    #[test]
    fn test_overflow_protection_cpu() {
        let calc = DefaultCostCalculator::new();
        let manifest = make_manifest(255, 100, u64::MAX);
        let pricing = make_pricing(u64::MAX, 0.0);

        let result = calc.estimate(&manifest, &pricing);
        assert!(matches!(result, Err(CostError::Overflow)));
    }

    #[test]
    fn test_overflow_protection_memory() {
        let calc = DefaultCostCalculator::new();
        let manifest = make_manifest(1, u32::MAX, u64::MAX);
        let pricing = make_pricing(0, f64::MAX);

        let result = calc.estimate(&manifest, &pricing);
        assert!(matches!(result, Err(CostError::Overflow)));
    }

    #[test]
    fn test_charge_policy_edge_cases() {
        let calc = DefaultCostCalculator::new();

        // Exit 0 = success
        assert_eq!(calc.get_charge_policy(0), (100, None));

        // Exit 127 = user error
        assert_eq!(calc.get_charge_policy(127), (100, None));

        // Exit 128 (signal kill) - treat as user error
        assert_eq!(calc.get_charge_policy(128), (100, None));

        // Exit 200 = worker crash
        assert_eq!(
            calc.get_charge_policy(200),
            (0, Some(RefundReason::WorkerCrash))
        );

        // Exit 201 = kernel panic
        assert_eq!(
            calc.get_charge_policy(201),
            (0, Some(RefundReason::WorkerCrash))
        );

        // Exit 202 = build failure
        assert_eq!(
            calc.get_charge_policy(202),
            (50, Some(RefundReason::BuildFailure))
        );

        // Exit 203+ = unknown, charge full
        assert_eq!(calc.get_charge_policy(203), (100, None));
    }
}
