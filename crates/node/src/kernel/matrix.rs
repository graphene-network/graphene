use super::types::{Architecture, KernelSpec, Runtime};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Root structure for kernel-matrix.toml
#[derive(Debug, Deserialize)]
pub struct KernelMatrix {
    /// Unikraft version used for builds
    pub unikraft_version: String,
    /// Default memory settings
    #[serde(default)]
    pub defaults: MatrixDefaults,
    /// Runtime configurations
    pub runtimes: HashMap<String, RuntimeConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MatrixDefaults {
    #[serde(default = "default_min_memory")]
    pub min_memory_mib: u16,
    #[serde(default = "default_recommended_memory")]
    pub recommended_memory_mib: u16,
    #[serde(default = "default_boot_args")]
    pub boot_args: String,
}

fn default_min_memory() -> u16 {
    128
}

fn default_recommended_memory() -> u16 {
    256
}

fn default_boot_args() -> String {
    "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules".to_string()
}

/// Configuration for a specific runtime
#[derive(Debug, Deserialize)]
pub struct RuntimeConfig {
    /// Versions to build
    pub versions: Vec<String>,
    /// Architectures to build for
    #[serde(default = "default_architectures")]
    pub architectures: Vec<String>,
    /// Optional variants (e.g., "minimal", "full")
    #[serde(default)]
    pub variants: Vec<String>,
    /// Runtime-specific memory override
    #[serde(default)]
    pub min_memory_mib: Option<u16>,
    /// Runtime-specific recommended memory
    #[serde(default)]
    pub recommended_memory_mib: Option<u16>,
    /// Runtime-specific boot args
    #[serde(default)]
    pub boot_args: Option<String>,
}

fn default_architectures() -> Vec<String> {
    vec!["x86_64".to_string()]
}

impl KernelMatrix {
    /// Load matrix from TOML file
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read matrix file: {}", e))?;
        Self::parse(&content)
    }

    /// Parse matrix from TOML string
    pub fn parse(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| format!("failed to parse matrix TOML: {}", e))
    }

    /// Generate all KernelSpecs defined in the matrix
    pub fn all_specs(&self) -> Vec<KernelSpec> {
        let mut specs = Vec::new();

        for (runtime_name, config) in &self.runtimes {
            let runtime: Runtime = match runtime_name.parse() {
                Ok(r) => r,
                Err(_) => continue, // Skip unknown runtimes
            };

            for version in &config.versions {
                for arch_str in &config.architectures {
                    let arch: Architecture = match arch_str.parse() {
                        Ok(a) => a,
                        Err(_) => continue,
                    };

                    if config.variants.is_empty() {
                        // No variants, just create base spec
                        specs.push(KernelSpec {
                            runtime,
                            version: version.clone(),
                            arch,
                            variant: None,
                        });
                    } else {
                        // Create spec for each variant
                        for variant in &config.variants {
                            specs.push(KernelSpec {
                                runtime,
                                version: version.clone(),
                                arch,
                                variant: Some(variant.clone()),
                            });
                        }
                    }
                }
            }
        }

        specs
    }

    /// Get memory configuration for a runtime
    pub fn get_memory_config(&self, runtime: &Runtime) -> (u16, u16) {
        let runtime_name = runtime.to_string();
        if let Some(config) = self.runtimes.get(&runtime_name) {
            let min = config
                .min_memory_mib
                .unwrap_or(self.defaults.min_memory_mib);
            let recommended = config
                .recommended_memory_mib
                .unwrap_or(self.defaults.recommended_memory_mib);
            (min, recommended)
        } else {
            (
                self.defaults.min_memory_mib,
                self.defaults.recommended_memory_mib,
            )
        }
    }

    /// Get boot args for a runtime
    pub fn get_boot_args(&self, runtime: &Runtime) -> String {
        let runtime_name = runtime.to_string();
        if let Some(config) = self.runtimes.get(&runtime_name) {
            config
                .boot_args
                .clone()
                .unwrap_or_else(|| self.defaults.boot_args.clone())
        } else {
            self.defaults.boot_args.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MATRIX: &str = r#"
unikraft_version = "0.17.0"

[defaults]
min_memory_mib = 128
recommended_memory_mib = 256
boot_args = "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules"

[runtimes.python]
versions = ["3.11", "3.12"]
architectures = ["x86_64", "aarch64"]

[runtimes.node]
versions = ["20", "22"]
architectures = ["x86_64"]
min_memory_mib = 256
recommended_memory_mib = 512

[runtimes.bun]
versions = ["1.x"]
architectures = ["x86_64"]

[runtimes.deno]
versions = ["2.x"]
architectures = ["x86_64"]
"#;

    #[test]
    fn test_parse_matrix() {
        let matrix = KernelMatrix::parse(TEST_MATRIX).unwrap();
        assert_eq!(matrix.unikraft_version, "0.17.0");
        assert_eq!(matrix.defaults.min_memory_mib, 128);
        assert!(matrix.runtimes.contains_key("python"));
        assert!(matrix.runtimes.contains_key("node"));
    }

    #[test]
    fn test_all_specs() {
        let matrix = KernelMatrix::parse(TEST_MATRIX).unwrap();
        let specs = matrix.all_specs();

        // Python: 2 versions * 2 architectures = 4
        // Node: 2 versions * 1 architecture = 2
        // Bun: 1 version * 1 architecture = 1
        // Deno: 1 version * 1 architecture = 1
        // Total: 8
        assert_eq!(specs.len(), 8);

        // Check Python specs
        let python_specs: Vec<_> = specs
            .iter()
            .filter(|s| s.runtime == Runtime::Python)
            .collect();
        assert_eq!(python_specs.len(), 4);

        // Check Node specs
        let node_specs: Vec<_> = specs
            .iter()
            .filter(|s| s.runtime == Runtime::Node)
            .collect();
        assert_eq!(node_specs.len(), 2);
    }

    #[test]
    fn test_memory_config_override() {
        let matrix = KernelMatrix::parse(TEST_MATRIX).unwrap();

        // Python uses defaults
        let (min, rec) = matrix.get_memory_config(&Runtime::Python);
        assert_eq!(min, 128);
        assert_eq!(rec, 256);

        // Node has overrides
        let (min, rec) = matrix.get_memory_config(&Runtime::Node);
        assert_eq!(min, 256);
        assert_eq!(rec, 512);
    }

    #[test]
    fn test_matrix_with_variants() {
        let toml = r#"
unikraft_version = "0.17.0"

[runtimes.python]
versions = ["3.11"]
architectures = ["x86_64"]
variants = ["minimal", "full"]
"#;
        let matrix = KernelMatrix::parse(toml).unwrap();
        let specs = matrix.all_specs();

        assert_eq!(specs.len(), 2);
        assert!(specs
            .iter()
            .any(|s| s.variant == Some("minimal".to_string())));
        assert!(specs.iter().any(|s| s.variant == Some("full".to_string())));
    }
}
