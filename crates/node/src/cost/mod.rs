//! Job cost calculation and tracking for the Graphene network.
//!
//! This module implements job cost estimation and charging per Whitepaper Section 7.2:
//!
//! ```text
//! max_cost = price_per_second * max_duration +
//!            price_per_gb * memory_gb +
//!            egress_price * estimated_egress
//! ```
//!
//! # Phased Implementation
//!
//! - **Phase 1**: CPU and memory cost calculation (this implementation)
//! - **Phase 2**: Egress metering (deferred to GitHub Issue #129)
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                    Cost Calculation Flow                   │
//! ├────────────────────────────────────────────────────────────┤
//! │  1. Job Request → estimate() → JobCostEstimate (max_cost) │
//! │  2. Verify ticket.amount >= max_cost                       │
//! │  3. Lock max_cost via CostTracker                          │
//! │  4. Execute job → ExecutionMetrics                         │
//! │  5. actual() → ActualJobCost (based on real usage)         │
//! │  6. settle() → JobCostSettlement (final charge + refund)   │
//! │  7. Unlock via CostTracker                                 │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Refund Policy Integration
//!
//! The settlement phase applies the refund policy based on exit codes:
//! - Exit 0: Success - charge 100% of actual_cost
//! - Exit 1-127: User code error - charge 100% of actual_cost
//! - Exit 200, 201: Worker crash - charge 0% (full refund)
//! - Exit 202: Build failure - charge 50% of actual_cost
//! - Timeout: Charge max_cost (user authorized full duration)

pub mod calculator;
pub mod tracker;
pub mod types;

#[cfg(test)]
pub mod mock;

pub use calculator::{CostCalculator, DefaultCostCalculator, ExecutionMetrics};
pub use tracker::CostTracker;
pub use types::{ActualJobCost, CostBreakdown, JobCostEstimate, JobCostSettlement, RefundReason};

/// Errors that can occur during cost operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CostError {
    /// Payment ticket does not authorize enough funds for estimated max cost.
    #[error("insufficient payment: required={required_micros}, provided={provided_micros}")]
    InsufficientPayment {
        required_micros: u64,
        provided_micros: u64,
    },

    /// Job not found in the cost tracker.
    #[error("job not found: {job_id}")]
    JobNotFound { job_id: String },

    /// Arithmetic overflow during cost calculation.
    #[error("overflow in cost calculation")]
    Overflow,

    /// Channel not found for cost tracking.
    #[error("channel not found: {channel_id:?}")]
    ChannelNotFound { channel_id: [u8; 32] },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::{JobManifest, WorkerPricing};
    use std::collections::HashMap;

    fn make_test_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30_000,
            kernel: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    fn make_test_pricing() -> WorkerPricing {
        WorkerPricing {
            cpu_ms_micros: 10, // 10 microtokens per CPU-ms
            memory_mb_ms_micros: 0.5,
            disk_gb_ms_micros: None,
            gpu_ms_micros: None,
            egress_mb_micros: None,
            ingress_mb_micros: None,
        }
    }

    #[test]
    fn test_cost_estimate_basic() {
        let calculator = DefaultCostCalculator::new();
        let manifest = make_test_manifest();
        let pricing = make_test_pricing();

        let estimate = calculator.estimate(&manifest, &pricing).unwrap();

        // CPU: 2 vcpu * 30000ms * 10 micros = 600,000
        // Memory: 512 MB * 30000ms * 0.5 micros = 7,680,000
        // Total: 8,280,000
        assert_eq!(estimate.cpu_cost_micros, 600_000);
        assert_eq!(estimate.memory_cost_micros, 7_680_000);
        assert_eq!(estimate.max_cost_micros, 8_280_000);
    }

    #[test]
    fn test_actual_cost_less_than_estimate() {
        let calculator = DefaultCostCalculator::new();
        let pricing = make_test_pricing();

        // Job ran for only 10 seconds instead of 30
        let metrics = ExecutionMetrics {
            actual_duration_ms: 10_000,
            vcpu: 2,
            memory_mb: 512,
            egress_bytes: 0,
            ingress_bytes: 0,
            exit_code: 0,
        };

        let actual = calculator.actual(&metrics, &pricing).unwrap();

        // CPU: 2 vcpu * 10000ms * 10 micros = 200,000
        // Memory: 512 MB * 10000ms * 0.5 micros = 2,560,000
        // Total: 2,760,000
        assert_eq!(actual.breakdown.cpu_cost_micros, 200_000);
        assert_eq!(actual.breakdown.memory_cost_micros, 2_560_000);
        assert_eq!(actual.total_cost_micros, 2_760_000);
    }

    #[test]
    fn test_settlement_full_charge() {
        let calculator = DefaultCostCalculator::new();
        let manifest = make_test_manifest();
        let pricing = make_test_pricing();

        let max_cost = calculator.estimate(&manifest, &pricing).unwrap();
        let metrics = ExecutionMetrics {
            actual_duration_ms: 10_000,
            vcpu: 2,
            memory_mb: 512,
            egress_bytes: 0,
            ingress_bytes: 0,
            exit_code: 0, // Success
        };
        let actual = calculator.actual(&metrics, &pricing).unwrap();

        let settlement = calculator.settle("job-1", [0u8; 32], &max_cost, &actual, 0);

        // Success = 100% charge
        assert_eq!(settlement.final_charge_micros, actual.total_cost_micros);
        assert_eq!(
            settlement.refund_micros,
            max_cost.max_cost_micros - actual.total_cost_micros
        );
    }

    #[test]
    fn test_settlement_worker_crash_full_refund() {
        let calculator = DefaultCostCalculator::new();
        let manifest = make_test_manifest();
        let pricing = make_test_pricing();

        let max_cost = calculator.estimate(&manifest, &pricing).unwrap();
        let metrics = ExecutionMetrics {
            actual_duration_ms: 5_000,
            vcpu: 2,
            memory_mb: 512,
            egress_bytes: 0,
            ingress_bytes: 0,
            exit_code: 200, // Worker crash
        };
        let actual = calculator.actual(&metrics, &pricing).unwrap();

        let settlement = calculator.settle("job-1", [0u8; 32], &max_cost, &actual, 200);

        // Worker crash = 0% charge, full refund
        assert_eq!(settlement.final_charge_micros, 0);
        assert_eq!(settlement.refund_micros, max_cost.max_cost_micros);
        assert_eq!(settlement.refund_reason, Some(RefundReason::WorkerCrash));
    }

    #[test]
    fn test_settlement_build_failure_partial() {
        let calculator = DefaultCostCalculator::new();
        let manifest = make_test_manifest();
        let pricing = make_test_pricing();

        let max_cost = calculator.estimate(&manifest, &pricing).unwrap();
        let metrics = ExecutionMetrics {
            actual_duration_ms: 5_000,
            vcpu: 2,
            memory_mb: 512,
            egress_bytes: 0,
            ingress_bytes: 0,
            exit_code: 202, // Build failure
        };
        let actual = calculator.actual(&metrics, &pricing).unwrap();

        let settlement = calculator.settle("job-1", [0u8; 32], &max_cost, &actual, 202);

        // Build failure = 50% of actual cost
        assert_eq!(settlement.final_charge_micros, actual.total_cost_micros / 2);
        assert_eq!(
            settlement.refund_micros,
            max_cost.max_cost_micros - actual.total_cost_micros / 2
        );
        assert_eq!(settlement.refund_reason, Some(RefundReason::BuildFailure));
    }
}
