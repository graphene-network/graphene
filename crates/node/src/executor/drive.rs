//! Execution drive builder for job execution.
//!
//! This module provides the infrastructure for creating ext4 disk images
//! that contain the job code, input, and configuration. These images are
//! mounted as the root filesystem for unikernel execution.
//!
//! # Drive Layout
//!
//! ```text
//! /app/           # User code (decrypted from code_hash) - read-only
//! /input/         # Input data (decrypted from input_hash) - read-only, optional
//! /output/        # Job output directory - writable
//! /etc/graphene/  # System config
//!   env.json    # Environment variables
//! ```
//!
//! # Environment Variables
//!
//! The env.json file contains merged environment variables:
//! - Reserved `GRAPHENE_*` variables (always set, cannot be overridden)
//! - User-provided variables from the job manifest
//!
//! Reserved variables take precedence over user-provided values with the same name.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::executor::types::{reserved_env, ExecutionError};
use crate::p2p::messages::JobManifest;

/// Paths used inside the execution drive.
pub mod paths {
    /// Directory containing user code.
    pub const APP_DIR: &str = "/app";
    /// Directory containing input data.
    pub const INPUT_DIR: &str = "/input";
    /// Directory for job output.
    pub const OUTPUT_DIR: &str = "/output";
    /// Directory for Graphene system configuration.
    pub const CONFIG_DIR: &str = "/etc/graphene";
    /// Path to the environment variables JSON file.
    pub const ENV_FILE: &str = "/etc/graphene/env.json";
}

/// Configuration for the execution drive builder.
#[derive(Debug, Clone)]
pub struct DriveConfig {
    /// Base directory for temporary drive files.
    pub work_dir: PathBuf,
    /// Size of the ext4 image in megabytes.
    pub image_size_mb: u32,
}

impl Default for DriveConfig {
    fn default() -> Self {
        Self {
            work_dir: PathBuf::from("/tmp/graphene-drives"),
            image_size_mb: 64,
        }
    }
}

/// Trait for building execution drives for job isolation.
///
/// Implementations are responsible for:
/// 1. Creating an ext4 filesystem image
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
    /// - `/etc/graphene/env.json` - Merged environment variables
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
/// Reserved `GRAPHENE_*` variables override any user-provided values.
///
/// # Format
///
/// ```json
/// {
///   "GRAPHENE_JOB_ID": "abc-123",
///   "GRAPHENE_INPUT_PATH": "/input",
///   "GRAPHENE_OUTPUT_PATH": "/output",
///   "GRAPHENE_TIMEOUT_MS": "30000",
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
        // Skip any user-provided GRAPHENE_* variables
        if !reserved_env::is_reserved(key) {
            env.insert(key.clone(), value.clone());
        }
    }

    // Add reserved environment variables (always override user values)
    env.insert(
        reserved_env::GRAPHENE_JOB_ID.to_string(),
        job_id.to_string(),
    );
    env.insert(
        reserved_env::GRAPHENE_INPUT_PATH.to_string(),
        paths::INPUT_DIR.to_string(),
    );
    env.insert(
        reserved_env::GRAPHENE_OUTPUT_PATH.to_string(),
        paths::OUTPUT_DIR.to_string(),
    );
    env.insert(
        reserved_env::GRAPHENE_TIMEOUT_MS.to_string(),
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
    use std::process::Command;
    use tokio::fs;

    /// Linux implementation of ExecutionDriveBuilder.
    ///
    /// Uses system tools (dd, mkfs.ext4, mount) to create and populate
    /// ext4 filesystem images.
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

        /// Create an empty ext4 image file.
        async fn create_image(&self, path: &Path, size_mb: u32) -> Result<(), ExecutionError> {
            // Create sparse file with dd
            let status = Command::new("dd")
                .args([
                    "if=/dev/zero",
                    &format!("of={}", path.display()),
                    "bs=1M",
                    &format!("count={}", size_mb),
                ])
                .output()
                .map_err(|e| ExecutionError::drive(format!("dd failed: {}", e)))?;

            if !status.status.success() {
                return Err(ExecutionError::drive(format!(
                    "dd failed: {}",
                    String::from_utf8_lossy(&status.stderr)
                )));
            }

            // Format as ext4
            let status = Command::new("mkfs.ext4")
                .args(["-F", "-q", &path.display().to_string()])
                .output()
                .map_err(|e| ExecutionError::drive(format!("mkfs.ext4 failed: {}", e)))?;

            if !status.status.success() {
                return Err(ExecutionError::drive(format!(
                    "mkfs.ext4 failed: {}",
                    String::from_utf8_lossy(&status.stderr)
                )));
            }

            Ok(())
        }

        /// Mount an ext4 image to a directory.
        async fn mount(&self, image: &Path, mount_point: &Path) -> Result<(), ExecutionError> {
            let status = Command::new("mount")
                .args([
                    "-o",
                    "loop",
                    &image.display().to_string(),
                    &mount_point.display().to_string(),
                ])
                .output()
                .map_err(|e| ExecutionError::drive(format!("mount failed: {}", e)))?;

            if !status.status.success() {
                return Err(ExecutionError::drive(format!(
                    "mount failed: {}",
                    String::from_utf8_lossy(&status.stderr)
                )));
            }

            Ok(())
        }

        /// Unmount a mount point.
        async fn unmount(&self, mount_point: &Path) -> Result<(), ExecutionError> {
            let status = Command::new("umount")
                .arg(mount_point.display().to_string())
                .output()
                .map_err(|e| ExecutionError::drive(format!("umount failed: {}", e)))?;

            if !status.status.success() {
                return Err(ExecutionError::drive(format!(
                    "umount failed: {}",
                    String::from_utf8_lossy(&status.stderr)
                )));
            }

            Ok(())
        }

        /// Extract a tarball to a directory.
        async fn extract_tarball(&self, tarball: &[u8], dest: &Path) -> Result<(), ExecutionError> {
            use std::io::Write;

            // Write tarball to temp file
            let tar_path = dest.with_extension("tar.gz");
            let mut file = std::fs::File::create(&tar_path).map_err(|e| {
                ExecutionError::drive(format!("failed to create temp tarball: {}", e))
            })?;
            file.write_all(tarball)
                .map_err(|e| ExecutionError::drive(format!("failed to write tarball: {}", e)))?;
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

            let image_path = self.config.work_dir.join(format!("{}.ext4", job_id));
            let mount_point = self.config.work_dir.join(format!("{}_mount", job_id));

            // Create the ext4 image
            self.create_image(&image_path, self.config.image_size_mb)
                .await?;

            // Create mount point
            fs::create_dir_all(&mount_point).await.map_err(|e| {
                ExecutionError::drive(format!("failed to create mount point: {}", e))
            })?;

            // Mount the image
            self.mount(&image_path, &mount_point).await?;

            // Create directory structure
            let app_dir = mount_point.join("app");
            let input_dir = mount_point.join("input");
            let output_dir = mount_point.join("output");
            let config_dir = mount_point.join("etc/graphene");

            // Use a closure to ensure cleanup on error
            let result = async {
                fs::create_dir_all(&app_dir)
                    .await
                    .map_err(|e| ExecutionError::drive(format!("failed to create /app: {}", e)))?;
                fs::create_dir_all(&input_dir).await.map_err(|e| {
                    ExecutionError::drive(format!("failed to create /input: {}", e))
                })?;
                fs::create_dir_all(&output_dir).await.map_err(|e| {
                    ExecutionError::drive(format!("failed to create /output: {}", e))
                })?;
                fs::create_dir_all(&config_dir).await.map_err(|e| {
                    ExecutionError::drive(format!("failed to create /etc/graphene: {}", e))
                })?;

                // Extract code tarball to /app
                self.extract_tarball(code, &app_dir).await?;

                // Extract input tarball to /input if provided
                if let Some(input_data) = input {
                    self.extract_tarball(input_data, &input_dir).await?;
                }

                // Write environment variables
                let env_json = build_env_json(job_id, user_env, manifest);
                let env_path = config_dir.join("env.json");
                fs::write(&env_path, &env_json).await.map_err(|e| {
                    ExecutionError::drive(format!("failed to write env.json: {}", e))
                })?;

                Ok::<(), ExecutionError>(())
            }
            .await;

            // Always unmount, even on error
            self.unmount(&mount_point).await?;

            // Clean up mount point directory
            let _ = fs::remove_dir(&mount_point).await;

            // Propagate any error from the inner operations
            result?;

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
                work_dir: std::env::temp_dir().join("graphene-mock-drives"),
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
            kernel: "python:3.12".to_string(),
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

        assert_eq!(parsed.get("GRAPHENE_JOB_ID"), Some(&"job-123".to_string()));
        assert_eq!(
            parsed.get("GRAPHENE_INPUT_PATH"),
            Some(&"/input".to_string())
        );
        assert_eq!(
            parsed.get("GRAPHENE_OUTPUT_PATH"),
            Some(&"/output".to_string())
        );
        assert_eq!(
            parsed.get("GRAPHENE_TIMEOUT_MS"),
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
        assert_eq!(parsed.get("GRAPHENE_JOB_ID"), Some(&"job-456".to_string()));

        // User vars are included
        assert_eq!(parsed.get("API_KEY"), Some(&"secret-key".to_string()));
        assert_eq!(parsed.get("DEBUG"), Some(&"true".to_string()));
    }

    #[test]
    fn test_build_env_json_reserved_vars_override_user() {
        let manifest = make_test_manifest();
        let mut user_env = HashMap::new();
        // User tries to override reserved variables
        user_env.insert("GRAPHENE_JOB_ID".to_string(), "malicious-id".to_string());
        user_env.insert("GRAPHENE_CUSTOM".to_string(), "custom-value".to_string());
        user_env.insert("SAFE_VAR".to_string(), "safe-value".to_string());

        let json = build_env_json("real-job-id", &user_env, &manifest);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();

        // Reserved var should have the real value, not the user's attempt
        assert_eq!(
            parsed.get("GRAPHENE_JOB_ID"),
            Some(&"real-job-id".to_string())
        );

        // User's GRAPHENE_* var should be filtered out
        assert!(!parsed.contains_key("GRAPHENE_CUSTOM"));

        // Safe user var should be included
        assert_eq!(parsed.get("SAFE_VAR"), Some(&"safe-value".to_string()));
    }

    #[test]
    fn test_paths_constants() {
        assert_eq!(paths::APP_DIR, "/app");
        assert_eq!(paths::INPUT_DIR, "/input");
        assert_eq!(paths::OUTPUT_DIR, "/output");
        assert_eq!(paths::CONFIG_DIR, "/etc/graphene");
        assert_eq!(paths::ENV_FILE, "/etc/graphene/env.json");
    }

    #[test]
    fn test_drive_config_default() {
        let config = DriveConfig::default();
        assert_eq!(config.work_dir, PathBuf::from("/tmp/graphene-drives"));
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

            assert_eq!(parsed.get("GRAPHENE_JOB_ID"), Some(&"job-env".to_string()));
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
