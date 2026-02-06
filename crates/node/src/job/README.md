# Job State Machine

This module implements the job lifecycle state machine per Whitepaper Section 5.7.

## State Diagram

```
PENDING → ACCEPTED → [BUILDING|CACHED] → RUNNING → [SUCCEEDED|FAILED|TIMEOUT]
                                                            ↓
                                                       DELIVERING
                                                            ↓
                                                   [DELIVERED|EXPIRED]
```

## States

| State | Description | Typical Duration |
|-------|-------------|------------------|
| `Pending` | Job submitted, awaiting worker acceptance | <100ms |
| `Accepted` | Worker accepted, determining build strategy | <10ms |
| `Building` | Cache miss, building unikernel | 1-60s |
| `Cached` | Cache hit, loading pre-built unikernel | <1ms |
| `Running` | MicroVM executing job | user-defined |
| `Succeeded` | Execution completed (exit code 0) | instant |
| `Failed` | Execution failed (exit code 1-127, 200-202) | instant |
| `Timeout` | Execution exceeded time limit (exit code 128) | instant |
| `Delivering` | Result blob available for P2P download | <24h |
| `Delivered` | User fetched result (terminal) | - |
| `Expired` | TTL passed without fetch (terminal) | - |

## User-Visible States

Internal states are mapped to simplified user-visible states:

| Internal State(s) | User State | Description |
|-------------------|------------|-------------|
| Pending | `pending` | Waiting to start |
| Accepted, Building, Cached | `starting` | Preparing to run |
| Running | `running` | Executing |
| Succeeded | `succeeded` | Completed successfully |
| Failed | `failed` | Execution failed |
| Timeout | `timeout` | Timed out |
| Delivering | `ready` | Result available |
| Delivered | `delivered` | Result fetched |
| Expired | `expired` | Result expired |

## Exit Codes

| Code | Constant | Meaning | User Refund | Worker Paid |
|------|----------|---------|-------------|-------------|
| 0 | `SUCCESS` | Job succeeded | 0% | 100% |
| 1-127 | `USER_ERROR_*` | User code error | 0% | 100% |
| 128 | `USER_TIMEOUT` | User timeout exceeded | 0% | 100% |
| 200 | `WORKER_CRASH` | Worker crashed | 100% | 0% |
| 201 | `WORKER_RESOURCE_EXHAUSTED` | Worker OOM/disk | 100% | 0% |
| 202 | `BUILD_FAILURE` | Dockerfile/Kraftfile error | 50% | 50% |

## Usage

```rust
use graphene_node::job::{Job, JobState, exit_code};

// Create a new job
let mut job = Job::new("job-123");
job.set_worker("worker-456");

// Transition through states
job.transition(JobState::Accepted)?;
job.transition(JobState::Building)?;  // or JobState::Cached
job.transition(JobState::Running)?;

// Complete with exit code
job.transition_with_exit_code(JobState::Succeeded, exit_code::SUCCESS)?;

// Deliver result
let result_hash = iroh_blobs::Hash::new(b"encrypted result");
job.transition_to_delivering(result_hash)?;
job.transition(JobState::Delivered)?;

// Check terminal state
assert!(job.is_terminal());

// Compute metrics
let metrics = job.compute_metrics();
println!("Total time: {}ms", metrics.total_ms.unwrap());
println!("Cache hit: {}", metrics.cache_hit);

// Get refund policy
let policy = job.refund_policy().unwrap();
assert_eq!(policy.worker_payment_percent, 100);
```

## Metrics

The `JobMetrics` struct provides timing information:

| Field | Description |
|-------|-------------|
| `queue_ms` | Time in Pending state |
| `build_ms` | Time in Building state (None if cache hit) |
| `boot_ms` | Time from Accepted to Running |
| `execution_ms` | Time in Running state |
| `total_ms` | End-to-end time |
| `cache_hit` | Whether Cached state was used |

## Error Handling

```rust
use graphene_node::job::JobError;

match job.transition(JobState::Running) {
    Ok(()) => println!("Transitioned successfully"),
    Err(JobError::InvalidTransition { from, to }) => {
        println!("Cannot go from {} to {}", from, to);
    }
    Err(JobError::TerminalState(state)) => {
        println!("Job already in terminal state: {}", state);
    }
    Err(JobError::ExitCodeRequired(state)) => {
        println!("Use transition_with_exit_code for {}", state);
    }
    _ => {}
}
```

## Module Structure

```
job/
├── mod.rs      # JobError, re-exports
├── state.rs    # JobState, UserJobState enums
├── types.rs    # Job, JobMetrics, RefundPolicy, StateTransition, exit_code
└── README.md   # This file
```
