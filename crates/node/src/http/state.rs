//! Shared application state for the HTTP API server.

use crate::executor::{ExecutionResult, JobExecutor};
use crate::job::Job;
use crate::types::WorkerCapabilities;
use crate::worker::WorkerStateMachine;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Shared state accessible by all HTTP handlers.
pub struct AppState {
    /// Job executor for running unikernel workloads.
    pub executor: Arc<dyn JobExecutor>,
    /// Worker lifecycle state machine.
    pub worker: Arc<WorkerStateMachine>,
    /// Worker capabilities (hardware/software).
    pub capabilities: Arc<WorkerCapabilities>,
    /// In-memory job store (job_id -> Job).
    pub jobs: Arc<RwLock<HashMap<String, Job>>>,
    /// Completed job results (job_id -> ExecutionResult).
    pub results: Arc<RwLock<HashMap<String, ExecutionResult>>>,
    /// Server start time for uptime calculation.
    pub start_time: Instant,
}

impl AppState {
    /// Create a new AppState with the given components.
    pub fn new(
        executor: Arc<dyn JobExecutor>,
        worker: Arc<WorkerStateMachine>,
        capabilities: WorkerCapabilities,
    ) -> Self {
        Self {
            executor,
            worker,
            capabilities: Arc::new(capabilities),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }

    /// Get server uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}
