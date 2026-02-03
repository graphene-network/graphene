//! Types for the worker discovery service.

use crate::p2p::messages::{
    WorkerCapabilities, WorkerLoad, WorkerPricing, WorkerRegion, WorkerReputation,
};
use iroh::{EndpointAddr, PublicKey};
use std::time::{Duration, Instant};

/// Configuration for the discovery service.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Interval between worker announcements.
    pub announce_interval: Duration,

    /// Interval between heartbeats.
    pub heartbeat_interval: Duration,

    /// Time after which a worker is considered offline.
    pub offline_threshold: Duration,

    /// Time after which an offline worker is removed.
    pub expiry_threshold: Duration,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            announce_interval: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(30),
            offline_threshold: Duration::from_secs(5 * 60),
            expiry_threshold: Duration::from_secs(60 * 60),
        }
    }
}

impl DiscoveryConfig {
    /// Create a config for fast testing with short intervals.
    pub fn for_testing() -> Self {
        Self {
            announce_interval: Duration::from_millis(50),
            heartbeat_interval: Duration::from_millis(50),
            offline_threshold: Duration::from_millis(100),
            expiry_threshold: Duration::from_millis(200),
        }
    }
}

/// Status of a discovered worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStatus {
    /// Worker is responding to heartbeats.
    Online,
    /// Worker has not responded within offline threshold.
    Offline,
}

/// Information about a discovered worker.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// The worker's node ID.
    pub node_id: PublicKey,

    /// The worker's network address (if known).
    pub addr: Option<EndpointAddr>,

    /// Worker software version.
    pub version: String,

    /// Worker hardware and software capabilities.
    pub capabilities: WorkerCapabilities,

    /// Worker pricing information.
    pub pricing: WorkerPricing,

    /// Current load status.
    pub load: WorkerLoad,

    /// Worker status (online/offline).
    pub status: WorkerStatus,

    /// When this worker was last seen.
    pub last_seen: Instant,

    /// Geographic regions where this worker operates.
    pub regions: Vec<WorkerRegion>,

    /// Reputation metrics for this worker.
    pub reputation: WorkerReputation,
}

impl WorkerInfo {
    /// Check if this worker meets the given requirements.
    pub fn meets_requirements(&self, requirements: &JobRequirements) -> bool {
        // Must be online
        if self.status != WorkerStatus::Online {
            return false;
        }

        // Must have available slots
        if self.load.available_slots == 0 {
            return false;
        }

        // Check resource requirements
        if self.capabilities.max_vcpu < requirements.vcpu {
            return false;
        }
        if self.capabilities.max_memory_mb < requirements.memory_mb {
            return false;
        }

        // Check kernel support
        if !self.capabilities.kernels.contains(&requirements.kernel) {
            return false;
        }

        // Check price if specified
        if let Some(max_price) = requirements.max_price_cpu_ms {
            if self.pricing.cpu_ms_micros > max_price {
                return false;
            }
        }

        // Check disk requirements
        if let Some(required_disk) = requirements.required_disk_gb {
            match &self.capabilities.disk {
                Some(disk) if disk.max_disk_gb >= required_disk => {}
                _ => return false,
            }
        }

        // Check GPU requirements
        if requirements.required_gpu && self.capabilities.gpus.is_empty() {
            return false;
        }

        // Note: preferred_regions is a preference, not a hard requirement
        // It can be used for sorting/ranking but doesn't filter workers out

        true
    }

    /// Check if this worker is in any of the preferred regions.
    pub fn in_preferred_regions(&self, preferred: &[String]) -> bool {
        if preferred.is_empty() {
            return true;
        }
        self.regions.iter().any(|r| preferred.contains(&r.country))
    }
}

/// Requirements for a job to match against workers.
#[derive(Debug, Clone, Default)]
pub struct JobRequirements {
    /// Required vCPUs.
    pub vcpu: u8,

    /// Required memory in MB.
    pub memory_mb: u32,

    /// Required kernel.
    pub kernel: String,

    /// Maximum acceptable CPU price per ms.
    pub max_price_cpu_ms: Option<u64>,

    /// Required disk space in GB.
    pub required_disk_gb: Option<u32>,

    /// Whether a GPU is required.
    pub required_gpu: bool,

    /// Preferred region country codes (ISO 3166-1 alpha-2).
    /// If specified, workers in these regions are preferred.
    pub preferred_regions: Option<Vec<String>>,
}
