//! Worker module for the Graphene compute network.
//!
//! This module provides the `Worker` struct and associated types for running
//! a Graphene worker node. Workers receive jobs over P2P, execute them in
//! Firecracker MicroVMs, and collect payment via Solana state channels.

mod config;
mod daemon;
mod error;
mod solana;

pub use config::{
    LoggingSettings, P2PSettings, SolanaSettings, VmmSettings, WorkerConfig, WorkerIdentity,
};
pub use daemon::{register_worker, run_daemon, show_status, unregister_worker};
pub use error::WorkerError;
pub use solana::{SolanaClient, WorkerStatus};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::p2p::GrapheneNode;

/// Statistics about the worker's current state.
#[derive(Debug, Default)]
pub struct WorkerStats {
    /// Number of jobs currently being processed
    pub active_jobs: AtomicU32,
    /// Total jobs completed since startup
    pub total_completed: AtomicU32,
    /// Total jobs failed since startup
    pub total_failed: AtomicU32,
}

impl WorkerStats {
    /// Create new stats with all counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment active job count.
    pub fn job_started(&self) {
        self.active_jobs.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active job count and increment completed count.
    pub fn job_completed(&self) {
        self.active_jobs.fetch_sub(1, Ordering::Relaxed);
        self.total_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active job count and increment failed count.
    pub fn job_failed(&self) {
        self.active_jobs.fetch_sub(1, Ordering::Relaxed);
        self.total_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current active job count.
    pub fn get_active(&self) -> u32 {
        self.active_jobs.load(Ordering::Relaxed)
    }
}

/// A Graphene compute worker.
///
/// The worker manages P2P networking, job processing, and Solana interactions.
pub struct Worker {
    /// Configuration for this worker
    config: WorkerConfig,

    /// P2P network node
    p2p: Option<Arc<GrapheneNode>>,

    /// Worker statistics
    stats: Arc<WorkerStats>,

    /// Whether the worker is running
    running: AtomicBool,

    /// Whether shutdown has been requested
    shutdown_requested: AtomicBool,
}

impl Worker {
    /// Create a new worker with the given configuration.
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            p2p: None,
            stats: Arc::new(WorkerStats::new()),
            running: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    /// Get the worker configuration.
    pub fn config(&self) -> &WorkerConfig {
        &self.config
    }

    /// Get the worker statistics.
    pub fn stats(&self) -> Arc<WorkerStats> {
        self.stats.clone()
    }

    /// Check if the worker is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Run the worker.
    ///
    /// This method blocks until shutdown is requested.
    pub async fn run(&mut self) -> Result<(), WorkerError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(WorkerError::AlreadyRunning);
        }

        // Use the daemon implementation
        let result = run_daemon(self.config.clone(), true).await;

        self.running.store(false, Ordering::SeqCst);
        result
    }

    /// Request graceful shutdown.
    pub fn shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
    }

    /// Get the P2P node if initialized.
    pub fn p2p(&self) -> Option<&Arc<GrapheneNode>> {
        self.p2p.as_ref()
    }
}
