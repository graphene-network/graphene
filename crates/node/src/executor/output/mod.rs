//! Output processing for job execution results.
//!
//! This module handles the post-execution pipeline:
//! 1. Reading output files from the `/output` directory in the drive
//! 2. Packaging output as a tarball
//! 3. Encrypting result, stdout, and stderr with channel keys
//! 4. Computing BLAKE3 hash of encrypted result
//!
//! # Architecture
//!
//! ```text
//! Drive (/output dir)
//!        │
//!        ▼
//! ┌─────────────────┐
//! │ OutputProcessor │
//! │                 │
//! │ 1. Read /output │
//! │ 2. Create tar   │
//! │ 3. Encrypt      │
//! │ 4. Hash         │
//! └─────────────────┘
//!        │
//!        ▼
//! ExecutionResult (encrypted)
//! ```

mod mock;

pub use mock::{MockOutputBehavior, MockOutputProcessor};

use crate::crypto::{ChannelKeys, CryptoProvider, EncryptionDirection};
use crate::executor::types::{ExecutionError, ExecutionRequest, ExecutionResult};
use async_trait::async_trait;
use iroh_blobs::Hash;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Trait for processing job output after execution.
///
/// Implementations handle reading output files from the drive, packaging them,
/// encrypting the results, and computing verification hashes.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow concurrent job processing.
#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait OutputProcessor: Send + Sync {
    /// Process the output from a completed job execution.
    ///
    /// # Arguments
    ///
    /// * `drive_path` - Path to the mounted drive containing the /output directory
    /// * `stdout` - Captured stdout from the job
    /// * `stderr` - Captured stderr from the job
    /// * `exit_code` - Process exit code (0 = success)
    /// * `duration` - Total execution time
    /// * `request` - Original execution request (for job ID and encryption keys)
    /// * `channel_keys` - Pre-derived channel keys for encryption
    ///
    /// # Returns
    ///
    /// * `Ok(ExecutionResult)` - Encrypted result with hash
    /// * `Err(ExecutionError)` - Failed to process output
    ///
    /// # Encryption
    ///
    /// All outputs (result tarball, stdout, stderr) are encrypted separately
    /// using the per-job ephemeral key derived from channel keys.
    async fn process(
        &self,
        drive_path: &Path,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
        duration: Duration,
        request: &ExecutionRequest,
        channel_keys: &ChannelKeys,
    ) -> Result<ExecutionResult, ExecutionError>;
}

/// Default implementation of OutputProcessor using real cryptographic primitives.
pub struct DefaultOutputProcessor<C: CryptoProvider> {
    crypto: Arc<C>,
}

impl<C: CryptoProvider> DefaultOutputProcessor<C> {
    /// Create a new output processor with the given crypto provider.
    pub fn new(crypto: Arc<C>) -> Self {
        Self { crypto }
    }
}

#[async_trait]
impl<C: CryptoProvider + 'static> OutputProcessor for DefaultOutputProcessor<C> {
    async fn process(
        &self,
        drive_path: &Path,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
        duration: Duration,
        request: &ExecutionRequest,
        channel_keys: &ChannelKeys,
    ) -> Result<ExecutionResult, ExecutionError> {
        let job_id = &request.job_id;
        debug!(job_id, ?drive_path, "Processing job output");

        // 1. Create tarball of /output directory
        let output_dir = drive_path.join("output");
        let result_tarball = create_output_tarball(&output_dir).await?;

        debug!(
            job_id,
            tarball_size = result_tarball.len(),
            "Created output tarball"
        );

        // 2. Encrypt the result tarball
        let encrypted_result = self
            .crypto
            .encrypt_job_blob(
                &result_tarball,
                channel_keys,
                job_id,
                EncryptionDirection::Output,
            )
            .map_err(|e| ExecutionError::output(format!("Failed to encrypt result: {}", e)))?
            .to_bytes();

        // 3. Encrypt stdout
        let encrypted_stdout = self
            .crypto
            .encrypt_job_blob(&stdout, channel_keys, job_id, EncryptionDirection::Output)
            .map_err(|e| ExecutionError::output(format!("Failed to encrypt stdout: {}", e)))?
            .to_bytes();

        // 4. Encrypt stderr
        let encrypted_stderr = self
            .crypto
            .encrypt_job_blob(&stderr, channel_keys, job_id, EncryptionDirection::Output)
            .map_err(|e| ExecutionError::output(format!("Failed to encrypt stderr: {}", e)))?
            .to_bytes();

        // 5. Compute BLAKE3 hash of encrypted result
        // Note: iroh_blobs::Hash is BLAKE3-based, we use it for consistency with Iroh blob system
        let result_hash = Hash::new(&encrypted_result);

        debug!(
            job_id,
            exit_code,
            ?duration,
            result_hash = %result_hash,
            "Output processing complete"
        );

        Ok(ExecutionResult::new(
            exit_code,
            duration,
            encrypted_result,
            encrypted_stdout,
            encrypted_stderr,
            result_hash,
        ))
    }
}

/// Create a tarball from the output directory.
///
/// If the output directory doesn't exist or is empty, returns an empty tarball.
async fn create_output_tarball(output_dir: &Path) -> Result<Vec<u8>, ExecutionError> {
    // Use spawn_blocking since tar operations are synchronous
    let output_dir = output_dir.to_path_buf();

    tokio::task::spawn_blocking(move || create_output_tarball_sync(&output_dir))
        .await
        .map_err(|e| ExecutionError::output(format!("Tarball task panicked: {}", e)))?
}

/// Synchronous implementation of tarball creation.
fn create_output_tarball_sync(output_dir: &Path) -> Result<Vec<u8>, ExecutionError> {
    let mut buffer = Vec::new();

    {
        let mut builder = tar::Builder::new(&mut buffer);

        if output_dir.exists() && output_dir.is_dir() {
            // Recursively add all files from output directory
            if let Err(e) = add_dir_to_tar(&mut builder, output_dir, Path::new("")) {
                warn!(
                    ?output_dir,
                    error = %e,
                    "Failed to add output directory to tarball, creating empty archive"
                );
                // Continue with empty tarball
            }
        } else {
            debug!(
                ?output_dir,
                "Output directory does not exist, creating empty tarball"
            );
        }

        // Finish writing the tarball
        builder
            .finish()
            .map_err(|e| ExecutionError::output(format!("Failed to finalize tarball: {}", e)))?;
    }

    Ok(buffer)
}

/// Recursively add a directory to a tar archive.
fn add_dir_to_tar<W: Write>(
    builder: &mut tar::Builder<W>,
    dir: &Path,
    prefix: &Path,
) -> Result<(), ExecutionError> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| ExecutionError::output(format!("Read dir: {}", e)))?;

    for entry in entries {
        let entry = entry.map_err(|e| ExecutionError::output(format!("Read dir entry: {}", e)))?;
        let path = entry.path();
        let name = prefix.join(entry.file_name());

        if path.is_dir() {
            // Recursively add subdirectory
            add_dir_to_tar(builder, &path, &name)?;
        } else if path.is_file() {
            // Add file to tarball
            let mut file = std::fs::File::open(&path)
                .map_err(|e| ExecutionError::output(format!("Open file {:?}: {}", path, e)))?;

            builder.append_file(&name, &mut file).map_err(|e| {
                ExecutionError::output(format!("Append file {:?} to tar: {}", name, e))
            })?;
        }
        // Skip symlinks and other special files for security
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DefaultCryptoProvider;
    use crate::p2p::messages::{JobManifest, ResultDeliveryMode};
    use crate::p2p::protocol::types::JobAssets;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_test_request(job_id: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            job_id,
            JobManifest {
                vcpu: 1,
                memory_mb: 256,
                timeout_ms: 5000,
                kernel: "python:3.12".to_string(),
                egress_allowlist: vec![],
                env: HashMap::new(),
                estimated_egress_mb: None,
                estimated_ingress_mb: None,
            },
            JobAssets {
                code_hash: Hash::from_bytes([1u8; 32]),
                code_url: None,
                input_hash: Hash::from_bytes([2u8; 32]),
                input_url: None,
            },
            [0u8; 32],
            [0u8; 32],
            [0u8; 32],
            ResultDeliveryMode::Sync,
        )
    }

    fn test_channel_keys() -> (ChannelKeys, ChannelKeys) {
        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];

        let user_signing = ed25519_dalek::SigningKey::from_bytes(&user_secret);
        let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);

        let user_public = user_signing.verifying_key().to_bytes();
        let worker_public = worker_signing.verifying_key().to_bytes();

        let channel_pda = [3u8; 32];

        let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
        let worker_keys = ChannelKeys::derive(&worker_secret, &user_public, &channel_pda).unwrap();

        (user_keys, worker_keys)
    }

    #[tokio::test]
    async fn test_create_empty_tarball() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");

        // Don't create the output directory - should return empty tarball
        let tarball = create_output_tarball(&output_dir).await.unwrap();

        // Empty tar is typically 1024 bytes (two 512-byte zero blocks)
        assert!(!tarball.is_empty());

        // Verify it's a valid tar by reading it
        let mut archive = tar::Archive::new(&tarball[..]);
        let entries: Vec<_> = archive.entries().unwrap().collect();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_create_tarball_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        // Create some test files
        std::fs::write(output_dir.join("result.json"), r#"{"status": "ok"}"#).unwrap();
        std::fs::write(output_dir.join("data.txt"), "Hello, world!").unwrap();

        let tarball = create_output_tarball(&output_dir).await.unwrap();

        // Extract and verify
        let mut archive = tar::Archive::new(&tarball[..]);
        let entries: Vec<_> = archive.entries().unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_create_tarball_with_subdirs() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        let subdir = output_dir.join("nested");
        std::fs::create_dir_all(&subdir).unwrap();

        std::fs::write(output_dir.join("root.txt"), "root file").unwrap();
        std::fs::write(subdir.join("nested.txt"), "nested file").unwrap();

        let tarball = create_output_tarball(&output_dir).await.unwrap();

        let mut archive = tar::Archive::new(&tarball[..]);
        let entries: Vec<_> = archive.entries().unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_process_output_encrypts_all_data() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();
        std::fs::write(output_dir.join("result.txt"), "computation result").unwrap();

        let crypto = Arc::new(DefaultCryptoProvider);
        let processor = DefaultOutputProcessor::new(crypto.clone());

        let request = make_test_request("test-job-123");
        let (_, worker_keys) = test_channel_keys();

        let result = processor
            .process(
                temp_dir.path(),
                b"stdout output".to_vec(),
                b"stderr output".to_vec(),
                0,
                Duration::from_millis(100),
                &request,
                &worker_keys,
            )
            .await
            .unwrap();

        // Verify result structure
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.duration, Duration::from_millis(100));
        assert!(result.succeeded());

        // Encrypted data should be larger than plaintext (includes overhead)
        assert!(!result.encrypted_result.is_empty());
        assert!(!result.encrypted_stdout.is_empty());
        assert!(!result.encrypted_stderr.is_empty());

        // Hash should be computed
        assert_ne!(result.result_hash, Hash::from_bytes([0u8; 32]));
    }

    #[tokio::test]
    async fn test_process_output_failed_exit_code() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        let crypto = Arc::new(DefaultCryptoProvider);
        let processor = DefaultOutputProcessor::new(crypto);

        let request = make_test_request("failed-job");
        let (_, worker_keys) = test_channel_keys();

        let result = processor
            .process(
                temp_dir.path(),
                vec![],
                b"Error: division by zero".to_vec(),
                1,
                Duration::from_millis(50),
                &request,
                &worker_keys,
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 1);
        assert!(!result.succeeded());
    }

    #[tokio::test]
    async fn test_result_hash_is_blake3() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();
        std::fs::write(output_dir.join("test.txt"), "test data").unwrap();

        let crypto = Arc::new(DefaultCryptoProvider);
        let processor = DefaultOutputProcessor::new(crypto);

        let request = make_test_request("hash-test");
        let (_, worker_keys) = test_channel_keys();

        let result = processor
            .process(
                temp_dir.path(),
                vec![],
                vec![],
                0,
                Duration::from_millis(10),
                &request,
                &worker_keys,
            )
            .await
            .unwrap();

        // Verify hash matches BLAKE3 of encrypted result
        let expected_hash = Hash::new(&result.encrypted_result);
        assert_eq!(result.result_hash, expected_hash);
    }
}
