//! Core types for job cost calculation.
//!
//! This module defines the cost-related types used throughout the system:
//! - [`JobCostEstimate`] - Maximum cost before execution (for payment validation)
//! - [`ActualJobCost`] - Actual cost after execution (based on real usage)
//! - [`CostBreakdown`] - Itemized cost components
//! - [`JobCostSettlement`] - Final settlement with refund calculation

use serde::{Deserialize, Serialize};

/// Estimated maximum cost before job execution.
///
/// This is calculated from the job manifest and worker pricing. The user's
/// payment ticket must authorize at least this amount for the job to be accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCostEstimate {
    /// Maximum total cost in microtokens.
    pub max_cost_micros: u64,

    /// CPU cost component: vcpu * timeout_ms * cpu_ms_micros
    pub cpu_cost_micros: u64,

    /// Memory cost component: memory_mb * timeout_ms * memory_mb_ms_micros
    pub memory_cost_micros: u64,

    /// Egress cost component (Phase 2): estimated_egress_mb * egress_mb_micros
    /// Currently always 0.
    pub egress_cost_micros: u64,

    /// GPU cost component (future): gpu_count * timeout_ms * gpu_ms_micros
    pub gpu_cost_micros: u64,
}

impl JobCostEstimate {
    /// Creates a new cost estimate with the given components.
    pub fn new(
        cpu_cost_micros: u64,
        memory_cost_micros: u64,
        egress_cost_micros: u64,
        gpu_cost_micros: u64,
    ) -> Self {
        let max_cost_micros =
            cpu_cost_micros + memory_cost_micros + egress_cost_micros + gpu_cost_micros;
        Self {
            max_cost_micros,
            cpu_cost_micros,
            memory_cost_micros,
            egress_cost_micros,
            gpu_cost_micros,
        }
    }

    /// Creates a zero-cost estimate.
    pub fn zero() -> Self {
        Self {
            max_cost_micros: 0,
            cpu_cost_micros: 0,
            memory_cost_micros: 0,
            egress_cost_micros: 0,
            gpu_cost_micros: 0,
        }
    }
}

/// Actual cost after job execution.
///
/// This is calculated from actual execution metrics and worker pricing.
/// Will always be <= the estimated max cost for the same job parameters
/// (since actual_duration <= timeout and actual_egress is measured).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualJobCost {
    /// Total actual cost in microtokens.
    pub total_cost_micros: u64,

    /// Itemized cost breakdown.
    pub breakdown: CostBreakdown,
}

impl ActualJobCost {
    /// Creates a new actual cost from a breakdown.
    pub fn new(breakdown: CostBreakdown) -> Self {
        Self {
            total_cost_micros: breakdown.total(),
            breakdown,
        }
    }

    /// Creates a zero-cost result.
    pub fn zero() -> Self {
        Self {
            total_cost_micros: 0,
            breakdown: CostBreakdown::zero(),
        }
    }
}

/// Itemized cost breakdown showing each component.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CostBreakdown {
    /// CPU cost: vcpu * duration_ms * cpu_ms_micros
    pub cpu_cost_micros: u64,

    /// Memory cost: memory_mb * duration_ms * memory_mb_ms_micros
    pub memory_cost_micros: u64,

    /// Egress cost: actual_egress_bytes / 1MB * egress_mb_micros
    pub egress_cost_micros: u64,

    /// Ingress cost: actual_ingress_bytes / 1MB * ingress_mb_micros
    #[serde(default)]
    pub ingress_cost_micros: u64,

    /// GPU cost (future): gpu_count * duration_ms * gpu_ms_micros
    pub gpu_cost_micros: u64,
}

impl CostBreakdown {
    /// Creates a new cost breakdown.
    pub fn new(
        cpu_cost_micros: u64,
        memory_cost_micros: u64,
        egress_cost_micros: u64,
        ingress_cost_micros: u64,
        gpu_cost_micros: u64,
    ) -> Self {
        Self {
            cpu_cost_micros,
            memory_cost_micros,
            egress_cost_micros,
            ingress_cost_micros,
            gpu_cost_micros,
        }
    }

    /// Creates a zero-cost breakdown.
    pub fn zero() -> Self {
        Self::default()
    }

    /// Returns the total cost from all components.
    pub fn total(&self) -> u64 {
        self.cpu_cost_micros
            .saturating_add(self.memory_cost_micros)
            .saturating_add(self.egress_cost_micros)
            .saturating_add(self.ingress_cost_micros)
            .saturating_add(self.gpu_cost_micros)
    }
}

/// Reason for a refund (partial or full).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefundReason {
    /// Job completed faster than max timeout - refund unused portion.
    EarlyCompletion,

    /// Worker crashed during execution - full refund.
    WorkerCrash,

    /// Kernel panic or VMM failure - full refund.
    KernelPanic,

    /// Build failed before execution - partial refund.
    BuildFailure,

    /// Job was cancelled - refund based on work completed.
    Cancelled,
}

impl std::fmt::Display for RefundReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefundReason::EarlyCompletion => write!(f, "early_completion"),
            RefundReason::WorkerCrash => write!(f, "worker_crash"),
            RefundReason::KernelPanic => write!(f, "kernel_panic"),
            RefundReason::BuildFailure => write!(f, "build_failure"),
            RefundReason::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Settlement record for a completed job.
///
/// Produced by the settlement phase, recording the final charge and any refund.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCostSettlement {
    /// Unique job identifier.
    pub job_id: String,

    /// Payment channel this settlement applies to.
    pub channel_id: [u8; 32],

    /// Maximum cost that was locked before execution.
    pub max_cost_micros: u64,

    /// Actual cost based on real resource usage.
    pub actual_cost_micros: u64,

    /// Final amount to charge the user.
    /// This may be less than actual_cost if refund policy applies.
    pub final_charge_micros: u64,

    /// Amount to refund (max_cost - final_charge).
    pub refund_micros: u64,

    /// Reason for refund, if any.
    pub refund_reason: Option<RefundReason>,

    /// Exit code of the job execution.
    pub exit_code: i32,

    /// Itemized cost breakdown.
    pub breakdown: CostBreakdown,
}

impl JobCostSettlement {
    /// Returns true if this settlement includes a refund.
    pub fn has_refund(&self) -> bool {
        self.refund_micros > 0
    }

    /// Returns the refund percentage (0-100).
    pub fn refund_percentage(&self) -> u8 {
        if self.max_cost_micros == 0 {
            return 0;
        }
        ((self.refund_micros as f64 / self.max_cost_micros as f64) * 100.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_cost_estimate_new() {
        let estimate = JobCostEstimate::new(100, 200, 50, 25);
        assert_eq!(estimate.max_cost_micros, 375);
        assert_eq!(estimate.cpu_cost_micros, 100);
        assert_eq!(estimate.memory_cost_micros, 200);
        assert_eq!(estimate.egress_cost_micros, 50);
        assert_eq!(estimate.gpu_cost_micros, 25);
    }

    #[test]
    fn test_job_cost_estimate_zero() {
        let estimate = JobCostEstimate::zero();
        assert_eq!(estimate.max_cost_micros, 0);
    }

    #[test]
    fn test_cost_breakdown_total() {
        let breakdown = CostBreakdown::new(100, 200, 50, 10, 25);
        assert_eq!(breakdown.total(), 385);
    }

    #[test]
    fn test_cost_breakdown_total_saturates() {
        let breakdown = CostBreakdown::new(u64::MAX, 1, 0, 0, 0);
        assert_eq!(breakdown.total(), u64::MAX);
    }

    #[test]
    fn test_actual_job_cost_new() {
        let breakdown = CostBreakdown::new(100, 200, 0, 0, 0);
        let actual = ActualJobCost::new(breakdown.clone());
        assert_eq!(actual.total_cost_micros, 300);
        assert_eq!(actual.breakdown, breakdown);
    }

    #[test]
    fn test_settlement_has_refund() {
        let settlement = JobCostSettlement {
            job_id: "test".to_string(),
            channel_id: [0u8; 32],
            max_cost_micros: 1000,
            actual_cost_micros: 800,
            final_charge_micros: 800,
            refund_micros: 200,
            refund_reason: Some(RefundReason::EarlyCompletion),
            exit_code: 0,
            breakdown: CostBreakdown::zero(),
        };
        assert!(settlement.has_refund());
    }

    #[test]
    fn test_settlement_refund_percentage() {
        let settlement = JobCostSettlement {
            job_id: "test".to_string(),
            channel_id: [0u8; 32],
            max_cost_micros: 1000,
            actual_cost_micros: 0,
            final_charge_micros: 0,
            refund_micros: 1000,
            refund_reason: Some(RefundReason::WorkerCrash),
            exit_code: 200,
            breakdown: CostBreakdown::zero(),
        };
        assert_eq!(settlement.refund_percentage(), 100);
    }

    #[test]
    fn test_settlement_refund_percentage_partial() {
        let settlement = JobCostSettlement {
            job_id: "test".to_string(),
            channel_id: [0u8; 32],
            max_cost_micros: 1000,
            actual_cost_micros: 500,
            final_charge_micros: 250,
            refund_micros: 750,
            refund_reason: Some(RefundReason::BuildFailure),
            exit_code: 202,
            breakdown: CostBreakdown::zero(),
        };
        assert_eq!(settlement.refund_percentage(), 75);
    }

    #[test]
    fn test_refund_reason_display() {
        assert_eq!(RefundReason::WorkerCrash.to_string(), "worker_crash");
        assert_eq!(RefundReason::BuildFailure.to_string(), "build_failure");
        assert_eq!(
            RefundReason::EarlyCompletion.to_string(),
            "early_completion"
        );
    }
}
