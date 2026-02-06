use graphene_node::cost::calculator::{DefaultCostCalculator, ExecutionMetrics};
use graphene_node::cost::{CostCalculator, CostTracker, RefundReason};
use graphene_node::p2p::messages::{JobManifest, WorkerPricing};

fn make_manifest() -> JobManifest {
    JobManifest {
        vcpu: 2,
        memory_mb: 512,
        timeout_ms: 1000,
        kernel: "python:3.12".to_string(),
        egress_allowlist: vec![],
        env: Default::default(),
        estimated_egress_mb: None,
        estimated_ingress_mb: None,
    }
}

#[test]
fn cost_tracker_end_to_end_settlement_flow() {
    let calculator = DefaultCostCalculator::new();
    let pricing = WorkerPricing::default();
    let tracker = CostTracker::new();
    let channel_id = [7u8; 32];

    let manifest = make_manifest();
    let estimate = calculator.estimate(&manifest, &pricing).unwrap();

    // Success path (early completion -> partial refund).
    tracker
        .lock("job-success", channel_id, estimate.clone())
        .unwrap();
    assert_eq!(
        tracker.locked_for_channel(&channel_id),
        estimate.max_cost_micros
    );

    let metrics = ExecutionMetrics::new(500, 2, 512, 0, 0, 0);
    let actual = calculator.actual(&metrics, &pricing).unwrap();
    let settlement = calculator.settle("job-success", channel_id, &estimate, &actual, 0);

    assert_eq!(settlement.final_charge_micros, actual.total_cost_micros);
    assert_eq!(
        settlement.refund_micros,
        estimate.max_cost_micros - actual.total_cost_micros
    );
    assert_eq!(
        settlement.refund_reason,
        Some(RefundReason::EarlyCompletion)
    );

    let unlocked = tracker.unlock("job-success").unwrap();
    assert_eq!(unlocked.max_cost_micros, estimate.max_cost_micros);
    assert_eq!(tracker.lock_count(), 0);

    // Worker crash (full refund).
    tracker
        .lock("job-crash", channel_id, estimate.clone())
        .unwrap();
    let metrics = ExecutionMetrics::new(200, 2, 512, 0, 0, 200);
    let actual = calculator.actual(&metrics, &pricing).unwrap();
    let settlement = calculator.settle("job-crash", channel_id, &estimate, &actual, 200);

    assert_eq!(settlement.final_charge_micros, 0);
    assert_eq!(settlement.refund_micros, estimate.max_cost_micros);
    assert_eq!(settlement.refund_reason, Some(RefundReason::WorkerCrash));
    tracker.unlock("job-crash").unwrap();

    // Timeout (charge max cost, no refund).
    tracker
        .lock("job-timeout", channel_id, estimate.clone())
        .unwrap();
    let metrics = ExecutionMetrics::new(1000, 2, 512, 0, 0, -1);
    let actual = calculator.actual(&metrics, &pricing).unwrap();
    let settlement = calculator.settle("job-timeout", channel_id, &estimate, &actual, -1);

    assert_eq!(settlement.final_charge_micros, estimate.max_cost_micros);
    assert_eq!(settlement.refund_micros, 0);
    assert_eq!(settlement.refund_reason, None);
    tracker.unlock("job-timeout").unwrap();
}
