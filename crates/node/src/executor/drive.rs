//! Execution drive builder for job execution.
//!
//! This module provides the infrastructure for creating CPIO initramfs images
//! that contain the job code, input, and configuration. These images are
//! used as the root filesystem for unikernel execution.
//!
//! # Implementation Notes
//!
//! The Linux implementation uses `cpio` to create initramfs archives without
//! requiring root privileges or mounting. This is critical for running in
//! unprivileged environments like CI runners or cloud VPS instances.
//!
//! # Drive Layout
//!
//! ```text
//! /app/           # User code (decrypted from code_hash) - read-only
//! /input/         # Input data (decrypted from input_hash) - read-only, optional
//! /output/        # Job output directory - writable
//! /etc/opencapsule/  # System config
//!   env.json    # Environment variables
//! ```
//!
//! # Environment Variables
//!
//! The env.json file contains merged environment variables:
//! - Reserved `OPENCAPSULE_*` variables (always set, cannot be overridden)
//! - User-provided variables from the job manifest
//!
//! Reserved variables take precedence over user-provided values with the same name.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::executor::types::{reserved_env, ExecutionError};
use crate::types::JobManifest;

/// Paths used inside the execution drive.
pub mod paths {
    /// Directory containing user code.
    pub const APP_DIR: &str = "/app";
    /// Directory containing input data.
    pub const INPUT_DIR: &str = "/input";
    /// Directory for job output.
    pub const OUTPUT_DIR: &str = "/output";
    /// Directory for OpenCapsule system configuration.
    pub const CONFIG_DIR: &str = "/etc/opencapsule";
    /// Path to the environment variables JSON file.
    pub const ENV_FILE: &str = "/etc/opencapsule/env.json";
}

/// Configuration for the execution drive builder.
#[derive(Debug, Clone)]
pub struct DriveConfig {
    /// Base directory for temporary drive files.
    pub work_dir: PathBuf,
    /// Size of the ext4 image in megabytes (legacy; initrd ignores this).
    pub image_size_mb: u32,
}

impl Default for DriveConfig {
    fn default() -> Self {
        Self {
            work_dir: PathBuf::from("/tmp/opencapsule-drives"),
            image_size_mb: 64,
        }
    }
}

/// Trait for building execution drives for job isolation.
///
/// Implementations are responsible for:
/// 1. Creating a CPIO initramfs image
/// 2. Extracting code and input tarballs to the appropriate directories
/// 3. Creating the output directory
/// 4. Writing the merged environment variables to env.json
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow concurrent drive preparation.
#[async_trait]
pub trait ExecutionDriveBuilder: Send + Sync {
    /// Prepare an execution drive for a job.
    ///
    /// Creates an ext4 filesystem image containing:
    /// - `/app/` - Extracted code tarball
    /// - `/input/` - Extracted input tarball (if provided)
    /// - `/output/` - Empty directory for job outputs
    /// - `/etc/opencapsule/env.json` - Merged environment variables
    ///
    /// # Arguments
    ///
    /// * `job_id` - Unique job identifier (used for naming the drive image)
    /// * `code` - Decrypted code tarball bytes (tar.gz format expected)
    /// * `input` - Optional decrypted input tarball bytes (tar.gz format expected)
    /// * `user_env` - User-provided environment variables from the manifest
    /// * `manifest` - Job manifest containing timeout and other configuration
    ///
    /// # Returns
    ///
    /// Path to the created ext4 image file.
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError::DriveFailed` if:
    /// - Failed to create the image file
    /// - Failed to format as ext4
    /// - Failed to mount/unmount
    /// - Failed to extract tarballs
    /// - Failed to write environment file
    async fn prepare(
        &self,
        job_id: &str,
        code: &[u8],
        input: Option<&[u8]>,
        user_env: &HashMap<String, String>,
        manifest: &JobManifest,
    ) -> Result<PathBuf, ExecutionError>;

    /// Clean up an execution drive after job completion.
    ///
    /// Removes the ext4 image file and any associated temporary files.
    ///
    /// # Arguments
    ///
    /// * `drive_path` - Path to the drive image to clean up
    ///
    /// # Errors
    ///
    /// Returns `ExecutionError::DriveFailed` if cleanup fails.
    async fn cleanup(&self, drive_path: &Path) -> Result<(), ExecutionError>;
}

/// Builds the merged environment variables JSON.
///
/// Reserved `OPENCAPSULE_*` variables override any user-provided values.
///
/// # Format
///
/// ```json
/// {
///   "OPENCAPSULE_JOB_ID": "abc-123",
///   "OPENCAPSULE_INPUT_PATH": "/input",
///   "OPENCAPSULE_OUTPUT_PATH": "/output",
///   "OPENCAPSULE_TIMEOUT_MS": "30000",
///   "USER_VAR": "user-value"
/// }
/// ```
pub fn build_env_json(
    job_id: &str,
    user_env: &HashMap<String, String>,
    manifest: &JobManifest,
) -> String {
    let mut env = HashMap::new();

    // Add user-provided environment variables first (will be overridden by reserved)
    for (key, value) in user_env {
        // Skip any user-provided OPENCAPSULE_* variables
        if !reserved_env::is_reserved(key) {
            env.insert(key.clone(), value.clone());
        }
    }

    // Add reserved environment variables (always override user values)
    env.insert(
        reserved_env::OPENCAPSULE_JOB_ID.to_string(),
        job_id.to_string(),
    );
    env.insert(
        reserved_env::OPENCAPSULE_INPUT_PATH.to_string(),
        paths::INPUT_DIR.to_string(),
    );
    env.insert(
        reserved_env::OPENCAPSULE_OUTPUT_PATH.to_string(),
        paths::OUTPUT_DIR.to_string(),
    );
    env.insert(
        reserved_env::OPENCAPSULE_TIMEOUT_MS.to_string(),
        manifest.timeout_ms.to_string(),
    );

    // Serialize to JSON
    serde_json::to_string_pretty(&env).expect("HashMap serialization should never fail")
}

// ============================================================================
// Linux Implementation
// ============================================================================

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use tokio::fs;
    use tracing::{debug, warn};

    /// Linux implementation of ExecutionDriveBuilder.
    ///
    /// Uses `cpio` to create initramfs archives without requiring root
    /// privileges or mounting. This is critical for running in unprivileged
    /// environments like CI runners.
    pub struct LinuxDriveBuilder {
        config: DriveConfig,
    }

    impl LinuxDriveBuilder {
        /// Create a new LinuxDriveBuilder with the given configuration.
        pub fn new(config: DriveConfig) -> Self {
            Self { config }
        }

        /// Create a new LinuxDriveBuilder with default configuration.
        pub fn with_defaults() -> Self {
            Self::new(DriveConfig::default())
        }

        /// Place asset contents into destination directory.
        ///
        /// - If bytes look like a gzip-compressed tar, extract it.
        /// - Otherwise, treat as a raw single-file payload and write to `fallback_name`.
        fn place_asset(
            &self,
            data: &[u8],
            dest: &Path,
            fallback_name: &str,
        ) -> Result<(), ExecutionError> {
            use std::io::Write;

            // Quick gzip magic check (1F 8B)
            let is_gzip = data.len() > 2 && data[0] == 0x1F && data[1] == 0x8B;

            if is_gzip {
                tracing::debug!(
                    dest = %dest.display(),
                    size = data.len(),
                    "Extracting gzip/tar asset to staging"
                );
                // Write tarball to temp file
                let tar_path = dest.with_extension("tar.gz");
                let mut file = std::fs::File::create(&tar_path).map_err(|e| {
                    ExecutionError::drive(format!("failed to create temp tarball: {}", e))
                })?;
                file.write_all(data).map_err(|e| {
                    ExecutionError::drive(format!("failed to write tarball: {}", e))
                })?;
                drop(file);

                // Extract with tar
                let status = Command::new("tar")
                    .args([
                        "-xzf",
                        &tar_path.display().to_string(),
                        "-C",
                        &dest.display().to_string(),
                    ])
                    .output()
                    .map_err(|e| ExecutionError::drive(format!("tar extract failed: {}", e)))?;

                // Clean up temp tarball
                let _ = std::fs::remove_file(&tar_path);

                if !status.status.success() {
                    return Err(ExecutionError::drive(format!(
                        "tar extract failed: {}",
                        String::from_utf8_lossy(&status.stderr)
                    )));
                }

                Ok(())
            } else {
                // Treat as raw single file
                let file_path = dest.join(fallback_name);
                tracing::debug!(
                    dest = %file_path.display(),
                    size = data.len(),
                    "Writing raw asset to staging"
                );
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ExecutionError::drive(format!(
                            "failed to create parent dirs for {}: {}",
                            file_path.display(),
                            e
                        ))
                    })?;
                }
                let mut file = std::fs::File::create(&file_path).map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to create raw asset file {}: {}",
                        file_path.display(),
                        e
                    ))
                })?;
                file.write_all(data).map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to write raw asset file {}: {}",
                        file_path.display(),
                        e
                    ))
                })?;
                Ok(())
            }
        }

        /// Create a CPIO initramfs from a staging directory.
        fn create_cpio_from_dir(
            &self,
            cpio_path: &Path,
            staging_dir: &Path,
        ) -> Result<(), ExecutionError> {
            debug!(
                image = %cpio_path.display(),
                staging = %staging_dir.display(),
                "Creating CPIO initramfs from staging directory"
            );

            let output = std::fs::File::create(cpio_path).map_err(|e| {
                ExecutionError::drive(format!(
                    "failed to create cpio image {}: {}",
                    cpio_path.display(),
                    e
                ))
            })?;

            let mut child = Command::new("cpio")
                .args(["--null", "-ov", "--format=newc"])
                .current_dir(staging_dir)
                .stdin(Stdio::piped())
                .stdout(output)
                .spawn()
                .map_err(|e| ExecutionError::drive(format!("cpio failed to spawn: {}", e)))?;

            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| ExecutionError::drive("cpio stdin unavailable".to_string()))?;

            let mut list = Vec::new();
            list.extend_from_slice(b".\0");
            self.append_cpio_paths(staging_dir, staging_dir, &mut list)?;

            stdin
                .write_all(&list)
                .map_err(|e| ExecutionError::drive(format!("cpio stdin write failed: {}", e)))?;
            drop(stdin);

            let status = child
                .wait()
                .map_err(|e| ExecutionError::drive(format!("cpio wait failed: {}", e)))?;
            if !status.success() {
                return Err(ExecutionError::drive(format!(
                    "cpio failed with status {}",
                    status
                )));
            }

            Ok(())
        }

        fn append_cpio_paths(
            &self,
            root: &Path,
            dir: &Path,
            out: &mut Vec<u8>,
        ) -> Result<(), ExecutionError> {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to read staging dir {}: {}",
                        dir.display(),
                        e
                    ))
                })?
                .collect();

            entries.sort_by_key(|entry| {
                entry
                    .as_ref()
                    .ok()
                    .and_then(|e| e.file_name().into_string().ok())
            });

            for entry in entries {
                let entry = entry.map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to read staging entry {}: {}",
                        dir.display(),
                        e
                    ))
                })?;
                let path = entry.path();
                let rel = path.strip_prefix(root).map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to relativize staging path {}: {}",
                        path.display(),
                        e
                    ))
                })?;

                let rel_str = rel.to_string_lossy();
                out.extend_from_slice(b"./");
                out.extend_from_slice(rel_str.as_bytes());
                out.push(0);

                let file_type = entry.file_type().map_err(|e| {
                    ExecutionError::drive(format!(
                        "failed to stat staging entry {}: {}",
                        path.display(),
                        e
                    ))
                })?;
                if file_type.is_dir() {
                    self.append_cpio_paths(root, &path, out)?;
                }
            }

            Ok(())
        }

        /// Clean up the staging directory, logging but not failing on errors.
        fn cleanup_staging(&self, staging_dir: &Path) {
            if let Err(e) = std::fs::remove_dir_all(staging_dir) {
                warn!(
                    staging = %staging_dir.display(),
                    error = %e,
                    "Failed to clean up staging directory"
                );
            }
        }
    }

    #[async_trait]
    impl ExecutionDriveBuilder for LinuxDriveBuilder {
        async fn prepare(
            &self,
            job_id: &str,
            code: &[u8],
            input: Option<&[u8]>,
            user_env: &HashMap<String, String>,
            manifest: &JobManifest,
        ) -> Result<PathBuf, ExecutionError> {
            // Ensure work directory exists
            fs::create_dir_all(&self.config.work_dir)
                .await
                .map_err(|e| ExecutionError::drive(format!("failed to create work dir: {}", e)))?;

            // Initrd is the only supported rootfs path with Unikraft + Firecracker.
            // Always build a CPIO initramfs so the kernel can mount it via vfs.fstab.
            let image_path = self.config.work_dir.join(format!("{}.cpio", job_id));
            let staging_dir = self.config.work_dir.join(format!("{}_staging", job_id));

            // Create staging directory structure mirroring the target filesystem
            let app_dir = staging_dir.join("app");
            let input_dir = staging_dir.join("input");
            let output_dir = staging_dir.join("output");
            let config_dir = staging_dir.join("etc/opencapsule");

            // Create all directories
            fs::create_dir_all(&app_dir)
                .await
                .map_err(|e| ExecutionError::drive(format!("failed to create /app: {}", e)))?;
            fs::create_dir_all(&input_dir)
                .await
                .map_err(|e| ExecutionError::drive(format!("failed to create /input: {}", e)))?;
            fs::create_dir_all(&output_dir)
                .await
                .map_err(|e| ExecutionError::drive(format!("failed to create /output: {}", e)))?;
            fs::create_dir_all(&config_dir).await.map_err(|e| {
                ExecutionError::drive(format!("failed to create /etc/opencapsule: {}", e))
            })?;

            // Choose filename based on kernel runtime
            let code_filename = if manifest.runtime.starts_with("python") {
                "main.py"
            } else if manifest.runtime.starts_with("node") {
                "index.js"
            } else {
                "code"
            };
            tracing::debug!(
                job_id,
                kernel = %manifest.runtime,
                filename = code_filename,
                staging = %app_dir.display(),
                "Placing code asset"
            );

            // Extract code asset to /app
            if let Err(e) = self.place_asset(code, &app_dir, code_filename) {
                self.cleanup_staging(&staging_dir);
                return Err(e);
            }

            // Extract input tarball to /input if provided
            if let Some(input_data) = input {
                if let Err(e) = self.place_asset(input_data, &input_dir, "input") {
                    self.cleanup_staging(&staging_dir);
                    return Err(e);
                }
            }

            // Write environment variables
            let env_json = build_env_json(job_id, user_env, manifest);
            let env_path = config_dir.join("env.json");
            if let Err(e) = fs::write(&env_path, &env_json).await {
                self.cleanup_staging(&staging_dir);
                return Err(ExecutionError::drive(format!(
                    "failed to write env.json: {}",
                    e
                )));
            }

            // Create the CPIO initramfs from the staging directory.
            if let Err(e) = self.create_cpio_from_dir(&image_path, &staging_dir) {
                self.cleanup_staging(&staging_dir);
                return Err(e);
            }

            // Clean up staging directory
            self.cleanup_staging(&staging_dir);

            Ok(image_path)
        }

        async fn cleanup(&self, drive_path: &Path) -> Result<(), ExecutionError> {
            if drive_path.exists() {
                fs::remove_file(drive_path)
                    .await
                    .map_err(|e| ExecutionError::drive(format!("failed to remove drive: {}", e)))?;
            }
            Ok(())
        }
    }
}

// ============================================================================
// Mock Implementation
// ============================================================================

pub mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Configurable behavior for the mock drive builder.
    #[derive(Debug, Clone, Default)]
    pub enum MockBehavior {
        /// Normal successful operation.
        #[default]
        HappyPath,
        /// Fail during image creation.
        ImageCreationFailure,
        /// Fail during tarball extraction.
        ExtractionFailure,
        /// Fail during cleanup.
        CleanupFailure,
    }

    /// Spy state to track mock interactions.
    #[derive(Debug, Default)]
    pub struct DriveBuilderSpyState {
        /// Job IDs for which prepare was called.
        pub prepare_calls: Vec<String>,
        /// Paths for which cleanup was called.
        pub cleanup_calls: Vec<PathBuf>,
        /// Last environment JSON that was "written".
        pub last_env_json: Option<String>,
        /// Last code tarball size.
        pub last_code_size: Option<usize>,
        /// Last input tarball size (if provided).
        pub last_input_size: Option<usize>,
    }

    /// Mock implementation of ExecutionDriveBuilder for testing.
    #[derive(Clone)]
    pub struct MockDriveBuilder {
        /// Configurable behavior.
        pub behavior: MockBehavior,
        /// Spy state for verification.
        pub spy: Arc<Mutex<DriveBuilderSpyState>>,
        /// Base directory for mock drive paths.
        pub work_dir: PathBuf,
    }

    impl Default for MockDriveBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockDriveBuilder {
        /// Create a new MockDriveBuilder with default (happy path) behavior.
        pub fn new() -> Self {
            Self {
                behavior: MockBehavior::HappyPath,
                spy: Arc::new(Mutex::new(DriveBuilderSpyState::default())),
                work_dir: std::env::temp_dir().join("opencapsule-mock-drives"),
            }
        }

        /// Create a MockDriveBuilder with specific behavior.
        pub fn with_behavior(behavior: MockBehavior) -> Self {
            Self {
                behavior,
                ..Self::new()
            }
        }

        /// Get the number of prepare calls.
        pub fn prepare_count(&self) -> usize {
            self.spy.lock().unwrap().prepare_calls.len()
        }

        /// Get the number of cleanup calls.
        pub fn cleanup_count(&self) -> usize {
            self.spy.lock().unwrap().cleanup_calls.len()
        }

        /// Check if prepare was called for a specific job ID.
        pub fn was_prepared(&self, job_id: &str) -> bool {
            self.spy
                .lock()
                .unwrap()
                .prepare_calls
                .contains(&job_id.to_string())
        }

        /// Get the last environment JSON that was generated.
        pub fn get_last_env_json(&self) -> Option<String> {
            self.spy.lock().unwrap().last_env_json.clone()
        }
    }

    #[async_trait]
    impl ExecutionDriveBuilder for MockDriveBuilder {
        async fn prepare(
            &self,
            job_id: &str,
            code: &[u8],
            input: Option<&[u8]>,
            user_env: &HashMap<String, String>,
            manifest: &JobManifest,
        ) -> Result<PathBuf, ExecutionError> {
            // Record the call
            {
                let mut spy = self.spy.lock().unwrap();
                spy.prepare_calls.push(job_id.to_string());
                spy.last_code_size = Some(code.len());
                spy.last_input_size = input.map(|i| i.len());
                spy.last_env_json = Some(build_env_json(job_id, user_env, manifest));
            }

            // Check for configured failures
            match &self.behavior {
                MockBehavior::ImageCreationFailure => {
                    return Err(ExecutionError::drive("mock: image creation failed"));
                }
                MockBehavior::ExtractionFailure => {
                    return Err(ExecutionError::drive("mock: tarball extraction failed"));
                }
                _ => {}
            }

            // Return a mock path
            let mock_path = self.work_dir.join(format!("{}.ext4", job_id));
            Ok(mock_path)
        }

        async fn cleanup(&self, drive_path: &Path) -> Result<(), ExecutionError> {
            // Record the call
            {
                let mut spy = self.spy.lock().unwrap();
                spy.cleanup_calls.push(drive_path.to_path_buf());
            }

            // Check for configured failures
            if matches!(self.behavior, MockBehavior::CleanupFailure) {
                return Err(ExecutionError::drive("mock: cleanup failed"));
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_manifest() -> JobManifest {
        JobManifest {
            vcpu: 2,
            memory_mb: 512,
            timeout_ms: 30000,
            runtime: "python:3.12".to_string(),
            egress_allowlist: vec![],
            env: HashMap::new(),
            estimated_egress_mb: None,
            estimated_ingress_mb: None,
        }
    }

    #[test]
    fn test_build_env_json_basic() {
        let manifest = make_test_manifest();
        let user_env = HashMap::new();

        let json = build_env_json("job-123", &user_env, &manifest);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.get("OPENCAPSULE_JOB_ID"), Some(&"job-123".to_string()));
        assert_eq!(
            parsed.get("OPENCAPSULE_INPUT_PATH"),
            Some(&"/input".to_string())
        );
        assert_eq!(
            parsed.get("OPENCAPSULE_OUTPUT_PATH"),
            Some(&"/output".to_string())
        );
        assert_eq!(
            parsed.get("OPENCAPSULE_TIMEOUT_MS"),
            Some(&"30000".to_string())
        );
    }

    #[test]
    fn test_build_env_json_with_user_vars() {
        let manifest = make_test_manifest();
        let mut user_env = HashMap::new();
        user_env.insert("API_KEY".to_string(), "secret-key".to_string());
        user_env.insert("DEBUG".to_string(), "true".to_string());

        let json = build_env_json("job-456", &user_env, &manifest);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

        // Reserved vars are present
        assert_eq!(parsed.get("OPENCAPSULE_JOB_ID"), Some(&"job-456".to_string()));

        // User vars are included
        assert_eq!(parsed.get("API_KEY"), Some(&"secret-key".to_string()));
        assert_eq!(parsed.get("DEBUG"), Some(&"true".to_string()));
    }

    #[test]
    fn test_build_env_json_reserved_vars_override_user() {
        let manifest = make_test_manifest();
        let mut user_env = HashMap::new();
        // User tries to override reserved variables
        user_env.insert("OPENCAPSULE_JOB_ID".to_string(), "malicious-id".to_string());
        user_env.insert("OPENCAPSULE_CUSTOM".to_string(), "custom-value".to_string());
        user_env.insert("SAFE_VAR".to_string(), "safe-value".to_string());

        let json = build_env_json("real-job-id", &user_env, &manifest);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

        // Reserved var should have the real value, not the user's attempt
        assert_eq!(
            parsed.get("OPENCAPSULE_JOB_ID"),
            Some(&"real-job-id".to_string())
        );

        // User's OPENCAPSULE_* var should be filtered out
        assert!(!parsed.contains_key("OPENCAPSULE_CUSTOM"));

        // Safe user var should be included
        assert_eq!(parsed.get("SAFE_VAR"), Some(&"safe-value".to_string()));
    }

    #[test]
    fn test_paths_constants() {
        assert_eq!(paths::APP_DIR, "/app");
        assert_eq!(paths::INPUT_DIR, "/input");
        assert_eq!(paths::OUTPUT_DIR, "/output");
        assert_eq!(paths::CONFIG_DIR, "/etc/opencapsule");
        assert_eq!(paths::ENV_FILE, "/etc/opencapsule/env.json");
    }

    #[test]
    fn test_drive_config_default() {
        let config = DriveConfig::default();
        assert_eq!(config.work_dir, PathBuf::from("/tmp/opencapsule-drives"));
        assert_eq!(config.image_size_mb, 64);
    }

    mod mock_tests {
        use super::*;
        use mock::{MockBehavior, MockDriveBuilder};

        #[tokio::test]
        async fn test_mock_happy_path() {
            let builder = MockDriveBuilder::new();
            let manifest = make_test_manifest();
            let user_env = HashMap::new();

            let result = builder
                .prepare("job-1", b"code-tarball", None, &user_env, &manifest)
                .await;

            assert!(result.is_ok());
            let path = result.unwrap();
            assert!(path.to_string_lossy().contains("job-1"));
            assert!(builder.was_prepared("job-1"));
            assert_eq!(builder.prepare_count(), 1);
        }

        #[tokio::test]
        async fn test_mock_with_input() {
            let builder = MockDriveBuilder::new();
            let manifest = make_test_manifest();
            let user_env = HashMap::new();

            let result = builder
                .prepare(
                    "job-2",
                    b"code-tarball",
                    Some(b"input-data"),
                    &user_env,
                    &manifest,
                )
                .await;

            assert!(result.is_ok());

            let spy = builder.spy.lock().unwrap();
            assert_eq!(spy.last_code_size, Some(12)); // "code-tarball".len()
            assert_eq!(spy.last_input_size, Some(10)); // "input-data".len()
        }

        #[tokio::test]
        async fn test_mock_image_creation_failure() {
            let builder = MockDriveBuilder::with_behavior(MockBehavior::ImageCreationFailure);
            let manifest = make_test_manifest();
            let user_env = HashMap::new();

            let result = builder
                .prepare("job-3", b"code", None, &user_env, &manifest)
                .await;

            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                ExecutionError::DriveFailed(_)
            ));
        }

        #[tokio::test]
        async fn test_mock_extraction_failure() {
            let builder = MockDriveBuilder::with_behavior(MockBehavior::ExtractionFailure);
            let manifest = make_test_manifest();
            let user_env = HashMap::new();

            let result = builder
                .prepare("job-4", b"code", None, &user_env, &manifest)
                .await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_cleanup_happy_path() {
            let builder = MockDriveBuilder::new();
            let path = PathBuf::from("/tmp/test.ext4");

            let result = builder.cleanup(&path).await;

            assert!(result.is_ok());
            assert_eq!(builder.cleanup_count(), 1);
        }

        #[tokio::test]
        async fn test_mock_cleanup_failure() {
            let builder = MockDriveBuilder::with_behavior(MockBehavior::CleanupFailure);
            let path = PathBuf::from("/tmp/test.ext4");

            let result = builder.cleanup(&path).await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_env_json_captured() {
            let builder = MockDriveBuilder::new();
            let manifest = make_test_manifest();
            let mut user_env = HashMap::new();
            user_env.insert("MY_VAR".to_string(), "my_value".to_string());

            let _ = builder
                .prepare("job-env", b"code", None, &user_env, &manifest)
                .await;

            let env_json = builder.get_last_env_json().unwrap();
            let parsed: HashMap<String, String> = serde_json::from_str(&env_json).unwrap();

            assert_eq!(parsed.get("OPENCAPSULE_JOB_ID"), Some(&"job-env".to_string()));
            assert_eq!(parsed.get("MY_VAR"), Some(&"my_value".to_string()));
        }

        #[tokio::test]
        async fn test_trait_is_object_safe() {
            // Verify the trait can be used as a trait object
            let builder: Box<dyn ExecutionDriveBuilder> = Box::new(MockDriveBuilder::new());
            let manifest = make_test_manifest();
            let user_env = HashMap::new();

            let result = builder
                .prepare("job-obj", b"code", None, &user_env, &manifest)
                .await;
            assert!(result.is_ok());
        }
    }
}
