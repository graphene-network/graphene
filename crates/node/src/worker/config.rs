//! Configuration types for the worker binary.

use crate::p2p::messages::{DiskCapability, DiskType, GpuCapability, WorkerRegion};
use crate::p2p::types::P2PConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::WorkerError;

/// Top-level worker configuration loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    /// Worker identity and capabilities
    pub worker: WorkerIdentity,

    /// P2P networking settings
    pub p2p: P2PSettings,

    /// Solana connection settings
    pub solana: SolanaSettings,

    /// VMM/Firecracker settings
    #[serde(default)]
    pub vmm: VmmSettings,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingSettings,
}

/// Worker identity and capability configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerIdentity {
    /// Human-readable name for this worker
    pub name: String,

    /// Capability tags (e.g., "gpu", "high-memory", "python")
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// Price per compute unit (in lamports)
    #[serde(default = "default_price")]
    pub price_per_unit: u64,

    /// Maximum job duration in seconds
    #[serde(default = "default_max_duration")]
    pub max_duration_secs: u64,

    /// Maximum concurrent job slots
    #[serde(default = "default_job_slots")]
    pub job_slots: u32,

    /// Disk storage configuration
    #[serde(default)]
    pub disk: Option<DiskConfig>,

    /// GPU configurations
    #[serde(default)]
    pub gpus: Vec<GpuConfig>,

    /// Geographic regions where this worker operates
    #[serde(default)]
    pub regions: Vec<RegionConfig>,
}

/// Disk storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    /// Maximum disk space in GB.
    pub max_disk_gb: u32,

    /// Type of disk storage (ssd, nvme, hdd).
    #[serde(default = "default_disk_type")]
    pub disk_type: String,

    /// Price per disk-GB-millisecond in microtokens.
    #[serde(default)]
    pub price_gb_ms_micros: Option<f64>,
}

fn default_disk_type() -> String {
    "ssd".to_string()
}

impl DiskConfig {
    /// Convert to the P2P message DiskCapability type.
    pub fn to_capability(&self) -> DiskCapability {
        let disk_type = match self.disk_type.to_lowercase().as_str() {
            "nvme" => DiskType::Nvme,
            "hdd" => DiskType::Hdd,
            _ => DiskType::Ssd,
        };
        DiskCapability {
            max_disk_gb: self.max_disk_gb,
            disk_type,
        }
    }
}

/// GPU configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    /// GPU model name (e.g., "NVIDIA RTX 4090").
    pub model: String,

    /// Video RAM in MB.
    pub vram_mb: u32,

    /// CUDA compute capability version (e.g., "8.9").
    #[serde(default)]
    pub compute_capability: Option<String>,

    /// Price per GPU-millisecond in microtokens.
    #[serde(default)]
    pub price_ms_micros: Option<u64>,
}

impl GpuConfig {
    /// Convert to the P2P message GpuCapability type.
    pub fn to_capability(&self) -> GpuCapability {
        GpuCapability {
            model: self.model.clone(),
            vram_mb: self.vram_mb,
            compute_capability: self.compute_capability.clone(),
        }
    }
}

/// Geographic region configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionConfig {
    /// ISO 3166-1 alpha-2 country code (e.g., "US", "DE").
    pub country: String,

    /// Optional cloud provider region (e.g., "us-east-1").
    #[serde(default)]
    pub cloud_region: Option<String>,
}

impl RegionConfig {
    /// Convert to the P2P message WorkerRegion type.
    pub fn to_region(&self) -> WorkerRegion {
        WorkerRegion {
            country: self.country.clone(),
            cloud_region: self.cloud_region.clone(),
        }
    }
}

fn default_price() -> u64 {
    1000 // 1000 lamports per unit
}

fn default_max_duration() -> u64 {
    300 // 5 minutes
}

fn default_job_slots() -> u32 {
    4
}

/// P2P networking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PSettings {
    /// Path for persistent P2P storage (identity, blobs)
    #[serde(default = "default_storage_path")]
    pub storage_path: PathBuf,

    /// Whether to use relay servers for NAT traversal
    #[serde(default = "default_true")]
    pub use_relay: bool,

    /// Port to bind to (0 for random)
    #[serde(default)]
    pub bind_port: u16,

    /// Bootstrap peers to connect to on startup
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
}

fn default_storage_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("graphene-worker")
}

fn default_true() -> bool {
    true
}

impl Default for P2PSettings {
    fn default() -> Self {
        Self {
            storage_path: default_storage_path(),
            use_relay: true,
            bind_port: 0,
            bootstrap_peers: Vec::new(),
        }
    }
}

/// Solana RPC and program configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaSettings {
    /// RPC endpoint URL
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,

    /// Path to the worker's keypair file
    pub keypair_path: PathBuf,

    /// Graphene program ID
    #[serde(default = "default_program_id")]
    pub program_id: String,
}

fn default_rpc_url() -> String {
    "https://api.devnet.solana.com".to_string()
}

fn default_program_id() -> String {
    "DHn6uXWDxnBJpkBhBFHiPoDe3S59EnrRQ9qb5rYUdHEs".to_string()
}

/// VMM/Firecracker configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmmSettings {
    /// Path to the firecracker binary
    #[serde(default = "default_firecracker_path")]
    pub firecracker_path: PathBuf,

    /// Directory for VM runtime files (sockets, logs)
    #[serde(default = "default_runtime_dir")]
    pub runtime_dir: PathBuf,

    /// Default vCPUs per VM
    #[serde(default = "default_vcpu")]
    pub default_vcpu: u8,

    /// Default memory per VM in MiB
    #[serde(default = "default_memory")]
    pub default_memory_mib: u16,

    /// Execution timeout in seconds
    #[serde(default = "default_execution_timeout")]
    pub execution_timeout_secs: u64,
}

fn default_firecracker_path() -> PathBuf {
    PathBuf::from("firecracker")
}

fn default_runtime_dir() -> PathBuf {
    PathBuf::from("/tmp/graphene-worker")
}

fn default_vcpu() -> u8 {
    2
}

fn default_memory() -> u16 {
    512
}

fn default_execution_timeout() -> u64 {
    300 // 5 minutes
}

impl Default for VmmSettings {
    fn default() -> Self {
        Self {
            firecracker_path: default_firecracker_path(),
            runtime_dir: default_runtime_dir(),
            default_vcpu: default_vcpu(),
            default_memory_mib: default_memory(),
            execution_timeout_secs: default_execution_timeout(),
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (pretty, json, compact)
    #[serde(default = "default_log_format")]
    pub format: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

impl WorkerConfig {
    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> Result<Self, WorkerError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            WorkerError::ConfigError(format!("Failed to read config file {:?}: {}", path, e))
        })?;

        let config: WorkerConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Convert P2P settings to the existing P2PConfig type.
    pub fn to_p2p_config(&self) -> P2PConfig {
        P2PConfig::new(&self.p2p.storage_path)
            .with_relay(self.p2p.use_relay)
            .with_port(self.p2p.bind_port)
    }
}
