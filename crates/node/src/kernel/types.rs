use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported runtime environments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Python,
    Node,
    Bun,
    Deno,
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Runtime::Python => write!(f, "python"),
            Runtime::Node => write!(f, "node"),
            Runtime::Bun => write!(f, "bun"),
            Runtime::Deno => write!(f, "deno"),
        }
    }
}

impl FromStr for Runtime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python" => Ok(Runtime::Python),
            "node" | "nodejs" => Ok(Runtime::Node),
            "bun" => Ok(Runtime::Bun),
            "deno" => Ok(Runtime::Deno),
            _ => Err(format!("unknown runtime: {}", s)),
        }
    }
}

/// CPU architecture for kernel builds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    #[default]
    X86_64,
    Aarch64,
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Architecture::X86_64 => write!(f, "x86_64"),
            Architecture::Aarch64 => write!(f, "aarch64"),
        }
    }
}

impl FromStr for Architecture {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "x86_64" | "x86-64" | "amd64" => Ok(Architecture::X86_64),
            "aarch64" | "arm64" => Ok(Architecture::Aarch64),
            _ => Err(format!("unknown architecture: {}", s)),
        }
    }
}

/// Specification for a kernel (used for lookups and requests)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KernelSpec {
    pub runtime: Runtime,
    pub version: String,
    #[serde(default)]
    pub arch: Architecture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl KernelSpec {
    pub fn new(runtime: Runtime, version: impl Into<String>) -> Self {
        Self {
            runtime,
            version: version.into(),
            arch: Architecture::default(),
            variant: None,
        }
    }

    pub fn with_arch(mut self, arch: Architecture) -> Self {
        self.arch = arch;
        self
    }

    pub fn with_variant(mut self, variant: impl Into<String>) -> Self {
        self.variant = Some(variant.into());
        self
    }

    /// Returns canonical name like "python-3.11-x86_64" or "python-3.11-minimal-aarch64"
    pub fn canonical_name(&self) -> String {
        let base = format!("{}-{}", self.runtime, self.version);
        match &self.variant {
            Some(v) => format!("{}-{}-{}", base, v, self.arch),
            None => format!("{}-{}", base, self.arch),
        }
    }

    /// Parse from string like "python-3.11", "node-20-minimal-aarch64"
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() < 2 {
            return Err(format!("invalid kernel spec: {}", s));
        }

        let runtime: Runtime = parts[0].parse()?;
        let version = parts[1].to_string();

        // Default values
        let mut arch = Architecture::default();
        let mut variant = None;

        // Parse remaining parts
        if parts.len() >= 3 {
            // Could be variant or architecture
            if let Ok(a) = parts[2].parse::<Architecture>() {
                arch = a;
            } else {
                variant = Some(parts[2].to_string());
                if parts.len() >= 4 {
                    arch = parts[3].parse()?;
                }
            }
        }

        Ok(KernelSpec {
            runtime,
            version,
            arch,
            variant,
        })
    }
}

impl fmt::Display for KernelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.canonical_name())
    }
}

impl FromStr for KernelSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Metadata about a specific kernel build
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelMetadata {
    pub spec: KernelSpec,
    /// BLAKE3 hash of the kernel binary
    pub binary_hash: String,
    /// Size of the kernel binary in bytes
    pub binary_size_bytes: u64,
    /// Minimum memory required to boot (MiB)
    pub min_memory_mib: u16,
    /// Recommended memory for typical workloads (MiB)
    pub recommended_memory_mib: u16,
    /// Default boot arguments for Firecracker
    pub default_boot_args: String,
    /// Unikraft version used to build this kernel
    pub unikraft_version: String,
    /// Build timestamp (RFC 3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_at: Option<String>,
}

impl KernelMetadata {
    /// Returns boot args with common defaults applied
    pub fn boot_args(&self) -> String {
        if self.default_boot_args.is_empty() {
            // Sensible defaults for unikernels
            "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules".to_string()
        } else {
            self.default_boot_args.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_display() {
        assert_eq!(Runtime::Python.to_string(), "python");
        assert_eq!(Runtime::Node.to_string(), "node");
        assert_eq!(Runtime::Bun.to_string(), "bun");
        assert_eq!(Runtime::Deno.to_string(), "deno");
    }

    #[test]
    fn test_runtime_parse() {
        assert_eq!("python".parse::<Runtime>().unwrap(), Runtime::Python);
        assert_eq!("node".parse::<Runtime>().unwrap(), Runtime::Node);
        assert_eq!("nodejs".parse::<Runtime>().unwrap(), Runtime::Node);
        assert_eq!("bun".parse::<Runtime>().unwrap(), Runtime::Bun);
        assert_eq!("deno".parse::<Runtime>().unwrap(), Runtime::Deno);
        assert!("unknown".parse::<Runtime>().is_err());
    }

    #[test]
    fn test_architecture_parse() {
        assert_eq!(
            "x86_64".parse::<Architecture>().unwrap(),
            Architecture::X86_64
        );
        assert_eq!(
            "amd64".parse::<Architecture>().unwrap(),
            Architecture::X86_64
        );
        assert_eq!(
            "aarch64".parse::<Architecture>().unwrap(),
            Architecture::Aarch64
        );
        assert_eq!(
            "arm64".parse::<Architecture>().unwrap(),
            Architecture::Aarch64
        );
    }

    #[test]
    fn test_kernel_spec_parse_simple() {
        let spec = KernelSpec::parse("python-3.11").unwrap();
        assert_eq!(spec.runtime, Runtime::Python);
        assert_eq!(spec.version, "3.11");
        assert_eq!(spec.arch, Architecture::X86_64);
        assert!(spec.variant.is_none());
    }

    #[test]
    fn test_kernel_spec_parse_with_arch() {
        let spec = KernelSpec::parse("node-20-aarch64").unwrap();
        assert_eq!(spec.runtime, Runtime::Node);
        assert_eq!(spec.version, "20");
        assert_eq!(spec.arch, Architecture::Aarch64);
        assert!(spec.variant.is_none());
    }

    #[test]
    fn test_kernel_spec_parse_with_variant() {
        let spec = KernelSpec::parse("python-3.12-minimal-x86_64").unwrap();
        assert_eq!(spec.runtime, Runtime::Python);
        assert_eq!(spec.version, "3.12");
        assert_eq!(spec.variant, Some("minimal".to_string()));
        assert_eq!(spec.arch, Architecture::X86_64);
    }

    #[test]
    fn test_kernel_spec_canonical_name() {
        let spec = KernelSpec::new(Runtime::Python, "3.11");
        assert_eq!(spec.canonical_name(), "python-3.11-x86_64");

        let spec = KernelSpec::new(Runtime::Node, "20")
            .with_arch(Architecture::Aarch64)
            .with_variant("minimal");
        assert_eq!(spec.canonical_name(), "node-20-minimal-aarch64");
    }

    #[test]
    fn test_kernel_spec_roundtrip() {
        let original = KernelSpec::new(Runtime::Bun, "1.x")
            .with_arch(Architecture::Aarch64)
            .with_variant("full");
        let canonical = original.canonical_name();
        let parsed = KernelSpec::parse(&canonical).unwrap();
        assert_eq!(original.runtime, parsed.runtime);
        assert_eq!(original.version, parsed.version);
        assert_eq!(original.arch, parsed.arch);
        assert_eq!(original.variant, parsed.variant);
    }
}
