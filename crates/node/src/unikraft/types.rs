use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Supported unikernel runtime environments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Runtime {
    Node20,
}

impl Runtime {
    /// Returns the Unikraft runtime identifier for Kraftfile
    pub fn as_kraft_runtime(&self) -> &'static str {
        match self {
            Runtime::Node20 => "node:20",
        }
    }

    /// Returns the expected base image for Dockerfiles
    pub fn base_image(&self) -> &'static str {
        match self {
            Runtime::Node20 => "graphene/node:20",
        }
    }
}

/// Resource limits for a build job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub vcpu: u8,
    pub memory_mib: u16,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            vcpu: 1,
            memory_mib: 256,
        }
    }
}

/// Manifest describing the build configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildManifest {
    pub runtime: Runtime,
    pub entrypoint: Vec<String>,
    pub resources: ResourceLimits,
}

/// A build job containing all inputs for unikernel compilation
#[derive(Debug, Clone)]
pub struct BuildJob {
    pub job_id: String,
    pub dockerfile: String,
    pub source_code: Vec<u8>, // tar bundle
    pub manifest: BuildManifest,
}

impl BuildJob {
    /// Create a new build job
    pub fn new(
        job_id: impl Into<String>,
        dockerfile: impl Into<String>,
        source_code: Vec<u8>,
        manifest: BuildManifest,
    ) -> Self {
        Self {
            job_id: job_id.into(),
            dockerfile: dockerfile.into(),
            source_code,
            manifest,
        }
    }

    /// Load a build job from the examples directory (for E2E testing)
    #[cfg(feature = "e2e")]
    pub fn from_example(name: &str) -> std::io::Result<Self> {
        use std::fs;
        use std::io::Read;

        let example_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples")
            .join(name);

        let dockerfile = fs::read_to_string(example_dir.join("Dockerfile"))?;

        // Create tar bundle of source files
        let mut tar_builder = tar::Builder::new(Vec::new());
        for entry in fs::read_dir(&example_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().unwrap().to_str().unwrap();
            // Skip Dockerfile and Kraftfile - they're handled separately
            if name == "Dockerfile" || name == "Kraftfile.yaml" || name == "README.md" {
                continue;
            }
            if path.is_file() {
                let mut file = fs::File::open(&path)?;
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)?;
                let mut header = tar::Header::new_gnu();
                header.set_path(name)?;
                header.set_size(contents.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar_builder.append(&header, contents.as_slice())?;
            }
        }
        let source_code = tar_builder.into_inner()?;

        Ok(Self {
            job_id: format!("example-{}", name),
            dockerfile,
            source_code,
            manifest: BuildManifest {
                runtime: Runtime::Node20,
                entrypoint: vec!["node".into(), "index.js".into()],
                resources: ResourceLimits::default(),
            },
        })
    }
}

/// The output of a successful unikernel build
#[derive(Debug, Clone)]
pub struct UnikernelImage {
    /// Blake3 hash of the unikernel binary
    pub hash: [u8; 32],
    /// Path to the cached unikernel file
    pub path: PathBuf,
    /// Size in bytes
    pub size_bytes: u64,
    /// Runtime the image was built for
    pub runtime: Runtime,
    /// When the image was built
    pub built_at: std::time::SystemTime,
}

/// Generated Kraftfile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kraftfile {
    pub spec: String,
    pub name: String,
    pub runtime: String,
    pub rootfs: String,
    pub cmd: Vec<String>,
}

impl Kraftfile {
    /// Create a new Kraftfile from a build manifest
    pub fn from_manifest(manifest: &BuildManifest, name: impl Into<String>) -> Self {
        Self {
            spec: "v0.6".to_string(),
            name: name.into(),
            runtime: manifest.runtime.as_kraft_runtime().to_string(),
            rootfs: "./Dockerfile".to_string(),
            cmd: manifest.entrypoint.clone(),
        }
    }

    /// Serialize to YAML format
    pub fn to_yaml(&self) -> String {
        format!(
            r#"spec: {}
name: {}
runtime: {}
rootfs: {}
cmd: [{}]
"#,
            self.spec,
            self.name,
            self.runtime,
            self.rootfs,
            self.cmd
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Configuration for the KraftBuilder
#[derive(Debug, Clone)]
pub struct KraftConfig {
    /// Path to the kraft CLI binary
    pub kraft_bin: PathBuf,
    /// Directory for caching built unikernels
    pub cache_dir: PathBuf,
    /// Maximum time allowed for a build
    pub build_timeout: Duration,
}

impl Default for KraftConfig {
    fn default() -> Self {
        Self {
            kraft_bin: PathBuf::from("kraft"),
            cache_dir: std::env::temp_dir().join("graphene-unikraft-cache"),
            build_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// A validated Dockerfile that has passed all checks
#[derive(Debug, Clone)]
pub struct ValidatedDockerfile {
    /// The original Dockerfile content
    pub content: String,
    /// Detected runtime from FROM instruction
    pub runtime: Runtime,
    /// Parsed CMD/ENTRYPOINT
    pub entrypoint: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kraftfile_to_yaml() {
        let manifest = BuildManifest {
            runtime: Runtime::Node20,
            entrypoint: vec!["node".into(), "index.js".into()],
            resources: ResourceLimits::default(),
        };
        let kraftfile = Kraftfile::from_manifest(&manifest, "test-app");
        let yaml = kraftfile.to_yaml();

        assert!(yaml.contains("spec: v0.6"));
        assert!(yaml.contains("name: test-app"));
        assert!(yaml.contains("runtime: node:20"));
        assert!(yaml.contains("rootfs: ./Dockerfile"));
        assert!(yaml.contains(r#"cmd: ["node", "index.js"]"#));
    }

    #[test]
    fn test_runtime_kraft_identifier() {
        assert_eq!(Runtime::Node20.as_kraft_runtime(), "node:20");
    }

    #[test]
    fn test_runtime_base_image() {
        assert_eq!(Runtime::Node20.base_image(), "graphene/node:20");
    }
}
