use crate::unikraft::{
    BuildJob, BuildManifest, Kraftfile, UnikernelBuilder, UnikernelImage, UnikraftError,
    ValidatedDockerfile,
};
use async_trait::async_trait;
use std::process::Stdio;
use std::time::SystemTime;
use tokio::process::Command;

pub use crate::unikraft::types::KraftConfig;

/// Real implementation of UnikernelBuilder using the kraft CLI
pub struct KraftBuilder {
    config: KraftConfig,
    validator: crate::unikraft::dockerfile::DockerfileValidator,
}

impl KraftBuilder {
    /// Create a new KraftBuilder with the given configuration
    pub fn new(config: KraftConfig) -> Self {
        Self {
            config,
            validator: crate::unikraft::dockerfile::DockerfileValidator::new(),
        }
    }

    /// Create a KraftBuilder with default configuration
    pub fn with_defaults() -> Self {
        Self::new(KraftConfig::default())
    }

    /// Extract a tar archive to a directory
    fn extract_tar(data: &[u8], dest: &std::path::Path) -> Result<(), UnikraftError> {
        use std::io::Cursor;

        let cursor = Cursor::new(data);
        let mut archive = tar::Archive::new(cursor);

        archive
            .unpack(dest)
            .map_err(|e: std::io::Error| UnikraftError::TarError(e.to_string()))?;

        Ok(())
    }

    /// Hash a file using blake3
    fn hash_file(path: &std::path::Path) -> Result<[u8; 32], UnikraftError> {
        let contents = std::fs::read(path)?;
        let hash = blake3::hash(&contents);
        Ok(*hash.as_bytes())
    }
}

#[async_trait]
impl UnikernelBuilder for KraftBuilder {
    async fn build(&self, job: &BuildJob) -> Result<UnikernelImage, UnikraftError> {
        // 1. Validate the Dockerfile
        let _validated = self.validate_dockerfile(&job.dockerfile)?;

        // 2. Create temp build directory
        let build_dir = std::env::temp_dir()
            .join("graphene-unikraft-builds")
            .join(&job.job_id);
        std::fs::create_dir_all(&build_dir)?;

        // 3. Write Dockerfile
        let dockerfile_path = build_dir.join("Dockerfile");
        std::fs::write(&dockerfile_path, &job.dockerfile)?;

        // 4. Extract source code tar
        Self::extract_tar(&job.source_code, &build_dir)?;

        // 5. Generate and write Kraftfile
        let kraftfile = self.generate_kraftfile(&job.manifest, &job.job_id);
        let kraftfile_path = build_dir.join("Kraftfile");
        std::fs::write(&kraftfile_path, kraftfile.to_yaml())?;

        // 6. Run kraft build with timeout
        let build_result = tokio::time::timeout(self.config.build_timeout, async {
            Command::new(&self.config.kraft_bin)
                .args(["build", "--plat", "fc", "--arch", "x86_64", "--no-cache"])
                .current_dir(&build_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        })
        .await;

        let output = match build_result {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                // Clean up build directory on error
                let _ = std::fs::remove_dir_all(&build_dir);
                return Err(UnikraftError::IoError(e));
            }
            Err(_) => {
                // Timeout - clean up and return error
                let _ = std::fs::remove_dir_all(&build_dir);
                return Err(UnikraftError::BuildTimeout {
                    elapsed: self.config.build_timeout,
                    limit: self.config.build_timeout,
                });
            }
        };

        // Check for build failure
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let _ = std::fs::remove_dir_all(&build_dir);
            return Err(UnikraftError::BuildFailed {
                exit_code: output.status.code().unwrap_or(-1),
                stderr,
            });
        }

        // 7. Find the output unikernel
        // kraft builds output to .unikraft/build/{name}_{plat}-{arch}
        let unik_path = build_dir
            .join(".unikraft")
            .join("build")
            .join(format!("{}_fc-x86_64", job.job_id));

        if !unik_path.exists() {
            // Try alternative paths
            let alt_path = build_dir.join(".unikraft").join("build");
            if alt_path.exists() {
                // Find any file in the build directory
                if let Ok(entries) = std::fs::read_dir(&alt_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            // Use this file
                            return self
                                .finalize_build(&path, &job.manifest.runtime, &build_dir)
                                .await;
                        }
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&build_dir);
            return Err(UnikraftError::BuildFailed {
                exit_code: 0,
                stderr: format!("Build output not found at {:?}", unik_path),
            });
        }

        self.finalize_build(&unik_path, &job.manifest.runtime, &build_dir)
            .await
    }

    fn validate_dockerfile(&self, dockerfile: &str) -> Result<ValidatedDockerfile, UnikraftError> {
        self.validator.validate(dockerfile)
    }

    fn generate_kraftfile(&self, manifest: &BuildManifest, name: &str) -> Kraftfile {
        Kraftfile::from_manifest(manifest, name)
    }
}

impl KraftBuilder {
    async fn finalize_build(
        &self,
        unik_path: &std::path::Path,
        runtime: &crate::unikraft::Runtime,
        build_dir: &std::path::Path,
    ) -> Result<UnikernelImage, UnikraftError> {
        // 8. Hash the output
        let hash = Self::hash_file(unik_path)?;

        // 9. Create cache directory if needed
        std::fs::create_dir_all(&self.config.cache_dir)?;

        // 10. Copy to cache with hash-based name
        let cache_filename = format!("{}.unik", hex::encode(hash));
        let cache_path = self.config.cache_dir.join(&cache_filename);
        std::fs::copy(unik_path, &cache_path)?;

        // 11. Get file size
        let metadata = std::fs::metadata(&cache_path)?;
        let size_bytes = metadata.len();

        // 12. Clean up build directory
        let _ = std::fs::remove_dir_all(build_dir);

        Ok(UnikernelImage {
            hash,
            path: cache_path,
            size_bytes,
            runtime: *runtime,
            built_at: SystemTime::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unikraft::{ResourceLimits, Runtime};
    use std::path::PathBuf;

    #[test]
    fn test_generate_kraftfile() {
        let builder = KraftBuilder::with_defaults();
        let manifest = BuildManifest {
            runtime: Runtime::Node20,
            entrypoint: vec!["node".to_string(), "index.js".to_string()],
            resources: ResourceLimits::default(),
        };

        let kraftfile = builder.generate_kraftfile(&manifest, "test-app");

        assert_eq!(kraftfile.spec, "v0.6");
        assert_eq!(kraftfile.name, "test-app");
        assert_eq!(kraftfile.runtime, "node:20");
        assert_eq!(kraftfile.rootfs, "./Dockerfile");
        assert_eq!(kraftfile.cmd, vec!["node", "index.js"]);
    }

    #[test]
    fn test_validate_valid_dockerfile() {
        let builder = KraftBuilder::with_defaults();
        let dockerfile = r#"
FROM graphene/node:20
WORKDIR /app
COPY package.json .
RUN npm install
COPY . .
CMD ["node", "index.js"]
"#;

        let result = builder.validate_dockerfile(dockerfile);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_dockerfile() {
        let builder = KraftBuilder::with_defaults();
        let dockerfile = r#"
FROM ubuntu:22.04
RUN apt-get update
CMD ["bash"]
"#;

        let result = builder.validate_dockerfile(dockerfile);
        assert!(result.is_err());
    }

    #[test]
    fn test_kraft_config_default() {
        let config = KraftConfig::default();
        assert_eq!(config.kraft_bin, PathBuf::from("kraft"));
        assert_eq!(config.build_timeout, std::time::Duration::from_secs(300));
    }
}
