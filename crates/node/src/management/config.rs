//! Node configuration for management API
//!
//! Defines the NodeConfig structure applied via graphenectl.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Node configuration applied via management API
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Staking configuration
    #[serde(default)]
    pub staking: StakingConfig,

    /// Resource limits
    #[serde(default)]
    pub resources: ResourceConfig,

    /// Pricing configuration
    #[serde(default)]
    pub pricing: PricingConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Node ID (ed25519 public key, auto-generated if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,

    /// Listen address for P2P and management
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,

    /// Public IP to advertise (auto-detected if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertise_addr: Option<String>,

    /// Geographic regions for job routing
    #[serde(default)]
    pub regions: Vec<String>,
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            node_id: None,
            listen_addr: default_listen_addr(),
            advertise_addr: None,
            regions: Vec::new(),
        }
    }
}

/// Staking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingConfig {
    /// Path to wallet file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet_path: Option<PathBuf>,

    /// Auto-register on boot
    #[serde(default)]
    pub auto_register: bool,

    /// Amount to stake in $GRAPHENE
    #[serde(default = "default_stake_amount")]
    pub stake_amount: u64,
}

fn default_stake_amount() -> u64 {
    100
}

impl Default for StakingConfig {
    fn default() -> Self {
        Self {
            wallet_path: None,
            auto_register: false,
            stake_amount: default_stake_amount(),
        }
    }
}

/// Resource configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Maximum vCPUs to allocate to jobs
    #[serde(default = "default_max_vcpu")]
    pub max_vcpu: u8,

    /// Maximum memory in MB to allocate to jobs
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,

    /// Supported job tiers
    #[serde(default = "default_tiers")]
    pub tiers: Vec<String>,

    /// Supported kernel runtimes
    #[serde(default = "default_kernels")]
    pub kernels: Vec<String>,
}

fn default_max_vcpu() -> u8 {
    4
}

fn default_max_memory_mb() -> u32 {
    8192
}

fn default_tiers() -> Vec<String> {
    vec!["standard".to_string()]
}

fn default_kernels() -> Vec<String> {
    vec!["python-3.12".to_string(), "node-21".to_string()]
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            max_vcpu: default_max_vcpu(),
            max_memory_mb: default_max_memory_mb(),
            tiers: default_tiers(),
            kernels: default_kernels(),
        }
    }
}

/// Pricing configuration (in micros = $0.000001)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    /// Price per CPU-millisecond in micros
    #[serde(default = "default_cpu_ms_micros")]
    pub cpu_ms_micros: f64,

    /// Price per MB-millisecond in micros
    #[serde(default = "default_memory_mb_ms_micros")]
    pub memory_mb_ms_micros: f64,

    /// Price per MB egress in micros
    #[serde(default = "default_egress_mb_micros")]
    pub egress_mb_micros: f64,
}

fn default_cpu_ms_micros() -> f64 {
    1.0
}

fn default_memory_mb_ms_micros() -> f64 {
    0.1
}

fn default_egress_mb_micros() -> f64 {
    10000.0
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            cpu_ms_micros: default_cpu_ms_micros(),
            memory_mb_ms_micros: default_memory_mb_ms_micros(),
            egress_mb_micros: default_egress_mb_micros(),
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (json or text)
    #[serde(default = "default_log_format")]
    pub format: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "json".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

impl NodeConfig {
    /// Load configuration from TOML file
    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::from_toml(&content)
    }

    /// Parse configuration from TOML string
    pub fn from_toml(toml_str: &str) -> Result<Self, ConfigError> {
        toml::from_str(toml_str).map_err(ConfigError::Toml)
    }

    /// Serialize to TOML string
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::TomlSer)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate listen address format
        if self
            .network
            .listen_addr
            .parse::<std::net::SocketAddr>()
            .is_err()
        {
            return Err(ConfigError::Validation(format!(
                "Invalid listen address: {}",
                self.network.listen_addr
            )));
        }

        // Validate resource limits
        if self.resources.max_vcpu == 0 || self.resources.max_vcpu > 64 {
            return Err(ConfigError::Validation(
                "max_vcpu must be between 1 and 64".to_string(),
            ));
        }

        if self.resources.max_memory_mb < 128 {
            return Err(ConfigError::Validation(
                "max_memory_mb must be at least 128".to_string(),
            ));
        }

        // Validate pricing (must be non-negative)
        if self.pricing.cpu_ms_micros < 0.0 {
            return Err(ConfigError::Validation(
                "cpu_ms_micros must be non-negative".to_string(),
            ));
        }

        Ok(())
    }
}

/// Configuration errors
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    TomlSer(toml::ser::Error),
    Validation(String),
}

impl std::error::Error for ConfigError {}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
            ConfigError::Toml(e) => write!(f, "TOML parse error: {}", e),
            ConfigError::TomlSer(e) => write!(f, "TOML serialization error: {}", e),
            ConfigError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(e: toml::ser::Error) -> Self {
        ConfigError::TomlSer(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.network.listen_addr, "0.0.0.0:9000");
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = NodeConfig::default();
        let toml_str = config.to_toml().unwrap();
        let parsed = NodeConfig::from_toml(&toml_str).unwrap();
        assert_eq!(config.network.listen_addr, parsed.network.listen_addr);
    }

    #[test]
    fn test_validation_valid() {
        let config = NodeConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_invalid_vcpu() {
        let mut config = NodeConfig::default();
        config.resources.max_vcpu = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
[network]
listen_addr = "0.0.0.0:9000"
regions = ["us-west-2"]

[resources]
max_vcpu = 8
max_memory_mb = 16384
"#;
        let config = NodeConfig::from_toml(toml_str).unwrap();
        assert_eq!(config.resources.max_vcpu, 8);
        assert_eq!(config.resources.max_memory_mb, 16384);
        assert_eq!(config.network.regions, vec!["us-west-2"]);
    }
}
