//! Validation logic for P2P message types.

use super::messages::{
    DiskCapability, GpuCapability, WorkerAnnouncement, WorkerCapabilities, WorkerPricing,
    WorkerRegion, WorkerReputation,
};
use thiserror::Error;

/// Errors that can occur during message validation.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ValidationError {
    /// Country code is not a valid ISO 3166-1 alpha-2 code.
    #[error("Invalid country code: {0} (must be 2 uppercase letters)")]
    InvalidCountryCode(String),

    /// vCPU count must be at least 1.
    #[error("max_vcpu must be >= 1, got {0}")]
    InvalidVcpu(u8),

    /// Memory must be at least 1 MB.
    #[error("max_memory_mb must be >= 1, got {0}")]
    InvalidMemory(u32),

    /// Kernels list cannot be empty.
    #[error("kernels list cannot be empty")]
    NoKernels,

    /// Disk capacity must be at least 1 GB.
    #[error("max_disk_gb must be >= 1, got {0}")]
    InvalidDiskSize(u32),

    /// GPU VRAM must be at least 1 MB.
    #[error("vram_mb must be >= 1, got {0}")]
    InvalidVram(u32),

    /// GPU model cannot be empty.
    #[error("GPU model cannot be empty")]
    EmptyGpuModel,

    /// Success rate must be between 0.0 and 1.0.
    #[error("success_rate must be 0.0..=1.0, got {0}")]
    InvalidSuccessRate(f64),

    /// Uptime percentage must be between 0.0 and 100.0.
    #[error("uptime_percentage must be 0.0..=100.0, got {0}")]
    InvalidUptimePercentage(f64),

    /// Latency percentiles must be ordered: p50 <= p95 <= p99.
    #[error("Latency percentiles must be ordered: p50 ({p50}) <= p95 ({p95}) <= p99 ({p99})")]
    InvalidLatencyOrder { p50: u64, p95: u64, p99: u64 },

    /// Jobs failed cannot exceed jobs completed + jobs failed.
    #[error(
        "jobs_failed ({failed}) cannot result in negative success count (completed: {completed})"
    )]
    InvalidJobCounts { completed: u64, failed: u64 },

    /// CPU pricing must be positive.
    #[error("cpu_ms_micros must be > 0")]
    InvalidCpuPricing,

    /// Memory pricing must be non-negative.
    #[error("memory_mb_ms_micros must be >= 0.0, got {0}")]
    InvalidMemoryPricing(f64),

    /// Disk pricing must be positive when specified.
    #[error("disk_gb_ms_micros must be > 0.0 when specified, got {0}")]
    InvalidDiskPricing(f64),

    /// GPU pricing must be positive when specified.
    #[error("gpu_ms_micros must be > 0 when specified")]
    InvalidGpuPricing,

    /// Version string cannot be empty.
    #[error("version string cannot be empty")]
    EmptyVersion,

    /// Nested validation error in capabilities.
    #[error("capabilities validation failed: {0}")]
    CapabilitiesError(Box<ValidationError>),

    /// Nested validation error in pricing.
    #[error("pricing validation failed: {0}")]
    PricingError(Box<ValidationError>),

    /// Nested validation error in region.
    #[error("region validation failed: {0}")]
    RegionError(Box<ValidationError>),

    /// Nested validation error in reputation.
    #[error("reputation validation failed: {0}")]
    ReputationError(Box<ValidationError>),

    /// Nested validation error in GPU.
    #[error("GPU validation failed: {0}")]
    GpuError(Box<ValidationError>),

    /// Nested validation error in disk.
    #[error("disk validation failed: {0}")]
    DiskError(Box<ValidationError>),
}

/// Trait for validating message types.
pub trait Validate {
    /// Validate this instance, returning an error if invalid.
    fn validate(&self) -> Result<(), ValidationError>;
}

impl Validate for WorkerRegion {
    fn validate(&self) -> Result<(), ValidationError> {
        // ISO 3166-1 alpha-2: exactly 2 uppercase ASCII letters
        if self.country.len() != 2 || !self.country.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(ValidationError::InvalidCountryCode(self.country.clone()));
        }
        Ok(())
    }
}

impl Validate for WorkerReputation {
    fn validate(&self) -> Result<(), ValidationError> {
        // Success rate must be 0.0..=1.0
        if !(0.0..=1.0).contains(&self.success_rate) {
            return Err(ValidationError::InvalidSuccessRate(self.success_rate));
        }

        // Uptime percentage must be 0.0..=100.0
        if !(0.0..=100.0).contains(&self.uptime_percentage) {
            return Err(ValidationError::InvalidUptimePercentage(
                self.uptime_percentage,
            ));
        }

        // Latency percentiles must be ordered
        if self.latency_p50_ms > self.latency_p95_ms || self.latency_p95_ms > self.latency_p99_ms {
            return Err(ValidationError::InvalidLatencyOrder {
                p50: self.latency_p50_ms,
                p95: self.latency_p95_ms,
                p99: self.latency_p99_ms,
            });
        }

        Ok(())
    }
}

impl Validate for DiskCapability {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.max_disk_gb == 0 {
            return Err(ValidationError::InvalidDiskSize(self.max_disk_gb));
        }
        Ok(())
    }
}

impl Validate for GpuCapability {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.model.is_empty() {
            return Err(ValidationError::EmptyGpuModel);
        }
        if self.vram_mb == 0 {
            return Err(ValidationError::InvalidVram(self.vram_mb));
        }
        Ok(())
    }
}

impl Validate for WorkerCapabilities {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.max_vcpu == 0 {
            return Err(ValidationError::InvalidVcpu(self.max_vcpu));
        }
        if self.max_memory_mb == 0 {
            return Err(ValidationError::InvalidMemory(self.max_memory_mb));
        }
        if self.kernels.is_empty() {
            return Err(ValidationError::NoKernels);
        }

        // Validate disk if present
        if let Some(ref disk) = self.disk {
            disk.validate()
                .map_err(|e| ValidationError::DiskError(Box::new(e)))?;
        }

        // Validate each GPU
        for gpu in &self.gpus {
            gpu.validate()
                .map_err(|e| ValidationError::GpuError(Box::new(e)))?;
        }

        Ok(())
    }
}

impl Validate for WorkerPricing {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.cpu_ms_micros == 0 {
            return Err(ValidationError::InvalidCpuPricing);
        }
        if self.memory_mb_ms_micros < 0.0 {
            return Err(ValidationError::InvalidMemoryPricing(
                self.memory_mb_ms_micros,
            ));
        }
        if let Some(disk_price) = self.disk_gb_ms_micros {
            if disk_price <= 0.0 {
                return Err(ValidationError::InvalidDiskPricing(disk_price));
            }
        }
        if let Some(gpu_price) = self.gpu_ms_micros {
            if gpu_price == 0 {
                return Err(ValidationError::InvalidGpuPricing);
            }
        }
        Ok(())
    }
}

impl Validate for WorkerAnnouncement {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.version.is_empty() {
            return Err(ValidationError::EmptyVersion);
        }

        self.capabilities
            .validate()
            .map_err(|e| ValidationError::CapabilitiesError(Box::new(e)))?;

        self.pricing
            .validate()
            .map_err(|e| ValidationError::PricingError(Box::new(e)))?;

        for region in &self.regions {
            region
                .validate()
                .map_err(|e| ValidationError::RegionError(Box::new(e)))?;
        }

        self.reputation
            .validate()
            .map_err(|e| ValidationError::ReputationError(Box::new(e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::messages::DiskType;

    #[test]
    fn test_valid_region() {
        let region = WorkerRegion {
            country: "US".to_string(),
            cloud_region: Some("us-east-1".to_string()),
        };
        assert!(region.validate().is_ok());
    }

    #[test]
    fn test_invalid_country_code_lowercase() {
        let region = WorkerRegion {
            country: "us".to_string(),
            cloud_region: None,
        };
        assert!(matches!(
            region.validate(),
            Err(ValidationError::InvalidCountryCode(_))
        ));
    }

    #[test]
    fn test_invalid_country_code_length() {
        let region = WorkerRegion {
            country: "USA".to_string(),
            cloud_region: None,
        };
        assert!(matches!(
            region.validate(),
            Err(ValidationError::InvalidCountryCode(_))
        ));
    }

    #[test]
    fn test_valid_reputation() {
        let rep = WorkerReputation {
            jobs_completed: 100,
            jobs_failed: 5,
            success_rate: 0.95,
            latency_p50_ms: 10,
            latency_p95_ms: 50,
            latency_p99_ms: 100,
            uptime_percentage: 99.9,
        };
        assert!(rep.validate().is_ok());
    }

    #[test]
    fn test_invalid_success_rate() {
        let rep = WorkerReputation {
            success_rate: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            rep.validate(),
            Err(ValidationError::InvalidSuccessRate(_))
        ));
    }

    #[test]
    fn test_invalid_latency_order() {
        let rep = WorkerReputation {
            latency_p50_ms: 100,
            latency_p95_ms: 50, // p95 < p50, invalid
            latency_p99_ms: 200,
            ..Default::default()
        };
        assert!(matches!(
            rep.validate(),
            Err(ValidationError::InvalidLatencyOrder { .. })
        ));
    }

    #[test]
    fn test_valid_disk_capability() {
        let disk = DiskCapability {
            max_disk_gb: 100,
            disk_type: DiskType::Nvme,
        };
        assert!(disk.validate().is_ok());
    }

    #[test]
    fn test_invalid_disk_size() {
        let disk = DiskCapability {
            max_disk_gb: 0,
            disk_type: DiskType::Ssd,
        };
        assert!(matches!(
            disk.validate(),
            Err(ValidationError::InvalidDiskSize(0))
        ));
    }

    #[test]
    fn test_valid_gpu_capability() {
        let gpu = GpuCapability {
            model: "NVIDIA RTX 4090".to_string(),
            vram_mb: 24576,
            compute_capability: Some("8.9".to_string()),
        };
        assert!(gpu.validate().is_ok());
    }

    #[test]
    fn test_invalid_gpu_empty_model() {
        let gpu = GpuCapability {
            model: "".to_string(),
            vram_mb: 8192,
            compute_capability: None,
        };
        assert!(matches!(
            gpu.validate(),
            Err(ValidationError::EmptyGpuModel)
        ));
    }

    #[test]
    fn test_capabilities_no_kernels() {
        let caps = WorkerCapabilities {
            max_vcpu: 4,
            max_memory_mb: 8192,
            kernels: vec![],
            disk: None,
            gpus: vec![],
        };
        assert!(matches!(caps.validate(), Err(ValidationError::NoKernels)));
    }

    #[test]
    fn test_pricing_zero_cpu() {
        let pricing = WorkerPricing {
            cpu_ms_micros: 0,
            memory_mb_ms_micros: 0.1,
            disk_gb_ms_micros: None,
            gpu_ms_micros: None,
        };
        assert!(matches!(
            pricing.validate(),
            Err(ValidationError::InvalidCpuPricing)
        ));
    }
}
