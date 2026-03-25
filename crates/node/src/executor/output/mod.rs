//! Output processing for job execution results.
//!
//! Reads output files from the /output directory, packages as a tarball,
//! and computes BLAKE3 hash.

mod mock;

pub use mock::{MockOutputBehavior, MockOutputProcessor};

use crate::executor::types::{ExecutionError, ExecutionResult};
use async_trait::async_trait;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, warn};

/// Trait for processing job output after execution.
#[async_trait]
pub trait OutputProcessor: Send + Sync {
    /// Process the output from a completed job execution.
    async fn process(
        &self,
        drive_path: &Path,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
        duration: Duration,
    ) -> Result<ExecutionResult, ExecutionError>;
}

/// Default implementation of OutputProcessor.
pub struct DefaultOutputProcessor;

impl DefaultOutputProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultOutputProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutputProcessor for DefaultOutputProcessor {
    async fn process(
        &self,
        drive_path: &Path,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: i32,
        duration: Duration,
    ) -> Result<ExecutionResult, ExecutionError> {
        debug!(?drive_path, "Processing job output");

        // Create tarball of /output directory
        let output_dir = drive_path.join("output");
        let result_tarball = create_output_tarball(&output_dir).await?;

        debug!(
            tarball_size = result_tarball.len(),
            "Created output tarball"
        );

        Ok(ExecutionResult::new(
            exit_code,
            duration,
            result_tarball,
            stdout,
            stderr,
        ))
    }
}

/// Create a tarball from the output directory.
async fn create_output_tarball(output_dir: &Path) -> Result<Vec<u8>, ExecutionError> {
    let output_dir = output_dir.to_path_buf();
    tokio::task::spawn_blocking(move || create_output_tarball_sync(&output_dir))
        .await
        .map_err(|e| ExecutionError::output(format!("Tarball task panicked: {}", e)))?
}

fn create_output_tarball_sync(output_dir: &Path) -> Result<Vec<u8>, ExecutionError> {
    let mut buffer = Vec::new();

    {
        let mut builder = tar::Builder::new(&mut buffer);

        if output_dir.exists() && output_dir.is_dir() {
            if let Err(e) = add_dir_to_tar(&mut builder, output_dir, Path::new("")) {
                warn!(
                    ?output_dir,
                    error = %e,
                    "Failed to add output directory to tarball, creating empty archive"
                );
            }
        } else {
            debug!(
                ?output_dir,
                "Output directory does not exist, creating empty tarball"
            );
        }

        builder
            .finish()
            .map_err(|e| ExecutionError::output(format!("Failed to finalize tarball: {}", e)))?;
    }

    Ok(buffer)
}

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
            add_dir_to_tar(builder, &path, &name)?;
        } else if path.is_file() {
            let mut file = std::fs::File::open(&path)
                .map_err(|e| ExecutionError::output(format!("Open file {:?}: {}", path, e)))?;

            builder.append_file(&name, &mut file).map_err(|e| {
                ExecutionError::output(format!("Append file {:?} to tar: {}", name, e))
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_empty_tarball() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");

        let tarball = create_output_tarball(&output_dir).await.unwrap();
        assert!(!tarball.is_empty());

        let mut archive = tar::Archive::new(&tarball[..]);
        let entries: Vec<_> = archive.entries().unwrap().collect();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_create_tarball_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();

        std::fs::write(output_dir.join("result.json"), r#"{"status": "ok"}"#).unwrap();
        std::fs::write(output_dir.join("data.txt"), "Hello, world!").unwrap();

        let tarball = create_output_tarball(&output_dir).await.unwrap();

        let mut archive = tar::Archive::new(&tarball[..]);
        let entries: Vec<_> = archive.entries().unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_process_output() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("output");
        std::fs::create_dir_all(&output_dir).unwrap();
        std::fs::write(output_dir.join("result.txt"), "computation result").unwrap();

        let processor = DefaultOutputProcessor::new();

        let result = processor
            .process(
                temp_dir.path(),
                b"stdout output".to_vec(),
                b"stderr output".to_vec(),
                0,
                Duration::from_millis(100),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.duration, Duration::from_millis(100));
        assert!(result.succeeded());
        assert!(!result.result.is_empty());
        assert_eq!(result.stdout, b"stdout output");
        assert_eq!(result.stderr, b"stderr output");
    }
}
