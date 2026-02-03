//! Worker discovery service using Iroh gossip.
//!
//! This module provides the [`WorkerDiscovery`] trait which abstracts worker
//! discovery operations, enabling mock implementations for testing.

use crate::p2p::messages::WorkerLoad;
use crate::p2p::P2PError;
use async_trait::async_trait;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub mod mock;
pub mod service;
pub mod types;

pub use mock::MockWorkerDiscovery;
pub use service::IrohWorkerDiscovery;
pub use types::{DiscoveryConfig, JobRequirements, WorkerInfo, WorkerStatus};

/// Errors that can occur during discovery operations.
#[derive(Debug)]
pub enum DiscoveryError {
    /// P2P networking error.
    P2P(P2PError),
    /// Serialization/deserialization error.
    Serialization(String),
    /// Service is not running.
    NotRunning,
    /// Service is already running.
    AlreadyRunning,
}

impl Error for DiscoveryError {}

impl Display for DiscoveryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryError::P2P(e) => write!(f, "P2P error: {}", e),
            DiscoveryError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            DiscoveryError::NotRunning => write!(f, "Discovery service is not running"),
            DiscoveryError::AlreadyRunning => write!(f, "Discovery service is already running"),
        }
    }
}

impl From<P2PError> for DiscoveryError {
    fn from(e: P2PError) -> Self {
        DiscoveryError::P2P(e)
    }
}

impl From<serde_json::Error> for DiscoveryError {
    fn from(e: serde_json::Error) -> Self {
        DiscoveryError::Serialization(e.to_string())
    }
}

/// The worker discovery trait.
///
/// Implementations discover workers via gossip, track their capabilities and load,
/// and allow querying for workers matching job requirements.
#[async_trait]
pub trait WorkerDiscovery: Send + Sync {
    /// Start the discovery service.
    ///
    /// Subscribes to the gossip topic and starts background tasks for
    /// announcements, heartbeats, and expiry checking.
    async fn start(&self) -> Result<(), DiscoveryError>;

    /// Stop the discovery service.
    ///
    /// Broadcasts a final heartbeat with zero available slots and stops
    /// all background tasks.
    async fn stop(&self) -> Result<(), DiscoveryError>;

    /// Find workers matching the given job requirements.
    ///
    /// Returns only online workers with available capacity that meet
    /// all requirements.
    async fn find_workers(&self, requirements: &JobRequirements) -> Vec<WorkerInfo>;

    /// List all known workers, including offline ones.
    async fn list_workers(&self) -> Vec<WorkerInfo>;

    /// Update our load status and broadcast a heartbeat.
    async fn update_load(&self, load: WorkerLoad) -> Result<(), DiscoveryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::{WorkerCapabilities, WorkerPricing, WorkerReputation};
    use iroh::SecretKey;
    use rand::RngCore;
    use std::time::Instant;

    fn make_worker_info(
        vcpu: u8,
        memory_mb: u32,
        kernels: Vec<String>,
        slots: u8,
        status: WorkerStatus,
    ) -> WorkerInfo {
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let key = SecretKey::from_bytes(&key_bytes);
        WorkerInfo {
            node_id: key.public(),
            addr: None,
            version: "0.1.0".to_string(),
            capabilities: WorkerCapabilities {
                max_vcpu: vcpu,
                max_memory_mb: memory_mb,
                kernels,
                disk: None,
                gpus: Vec::new(),
            },
            pricing: WorkerPricing {
                cpu_ms_micros: 1,
                memory_mb_ms_micros: 0.1,
                disk_gb_ms_micros: None,
                gpu_ms_micros: None,
                egress_mb_micros: None,
            },
            load: WorkerLoad {
                available_slots: slots,
                queue_depth: 0,
            },
            status,
            last_seen: Instant::now(),
            regions: Vec::new(),
            reputation: WorkerReputation::default(),
        }
    }

    #[test]
    fn test_worker_meets_requirements_basic() {
        let worker = make_worker_info(
            8,
            16384,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_insufficient_vcpu() {
        let worker = make_worker_info(
            4,
            16384,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );

        let requirements = JobRequirements {
            vcpu: 8,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_insufficient_memory() {
        let worker = make_worker_info(
            8,
            4096,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_missing_kernel() {
        let worker = make_worker_info(
            8,
            16384,
            vec!["python-3.11-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_offline() {
        let worker = make_worker_info(
            8,
            16384,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Offline,
        );

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_no_slots() {
        let worker = make_worker_info(
            8,
            16384,
            vec!["node-20-unikraft".to_string()],
            0,
            WorkerStatus::Online,
        );

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: None,
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_price_too_high() {
        let mut worker = make_worker_info(
            8,
            16384,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );
        worker.pricing.cpu_ms_micros = 100;

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: Some(50),
            ..Default::default()
        };

        assert!(!worker.meets_requirements(&requirements));
    }

    #[test]
    fn test_worker_price_acceptable() {
        let mut worker = make_worker_info(
            8,
            16384,
            vec!["node-20-unikraft".to_string()],
            4,
            WorkerStatus::Online,
        );
        worker.pricing.cpu_ms_micros = 50;

        let requirements = JobRequirements {
            vcpu: 4,
            memory_mb: 8192,
            kernel: "node-20-unikraft".to_string(),
            max_price_cpu_ms: Some(100),
            ..Default::default()
        };

        assert!(worker.meets_requirements(&requirements));
    }
}
