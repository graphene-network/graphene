//! Drive preparation and artifact extraction for ephemeral builds.
//!
//! Handles creation of ext4 filesystems for input (code) and output (artifacts) drives,
//! as well as extraction of build results.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;
use tracing::{debug, error, info};

use super::BuildError;

/// Helper functions for drive preparation and artifact extraction.
pub struct DriveHelper {
    /// Directory for temporary drive files
    work_dir: PathBuf,
}

impl DriveHelper {
    /// Create a new drive helper with the specified work directory.
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    /// Get the work directory path.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Create an ext4 filesystem image of the specified size.
    ///
    /// Returns the path to the created image file.
    pub async fn create_ext4_image(
        &self,
        name: &str,
        size_mib: u32,
    ) -> Result<PathBuf, BuildError> {
        let image_path = self.work_dir.join(format!("{}.ext4", name));

        // Ensure work directory exists
        fs::create_dir_all(&self.work_dir)
            .await
            .map_err(|e| BuildError::DriveError(format!("Failed to create work dir: {}", e)))?;

        // Create sparse file with dd
        let output = Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", image_path.display()),
                "bs=1M",
                &format!("count={}", size_mib),
            ])
            .output()
            .map_err(|e| BuildError::DriveError(format!("dd failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!("dd failed: {}", stderr)));
        }

        // Format as ext4
        let output = Command::new("mkfs.ext4")
            .args(["-F", "-q", image_path.to_str().unwrap()])
            .output()
            .map_err(|e| BuildError::DriveError(format!("mkfs.ext4 failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!(
                "mkfs.ext4 failed: {}",
                stderr
            )));
        }

        debug!(
            "Created ext4 image: {} ({} MiB)",
            image_path.display(),
            size_mib
        );
        Ok(image_path)
    }

    /// Prepare the input drive with Dockerfile, Kraftfile, and code.
    ///
    /// Creates an ext4 image containing:
    /// - `/Dockerfile`
    /// - `/Kraftfile` (optional)
    /// - `/code/` (extracted from tarball)
    pub async fn prepare_input_drive(
        &self,
        build_id: &str,
        dockerfile: &str,
        kraftfile: Option<&str>,
        code_tarball: &Path,
        size_mib: u32,
    ) -> Result<PathBuf, BuildError> {
        let image_name = format!("input-{}", build_id);
        let image_path = self.create_ext4_image(&image_name, size_mib).await?;

        // Create mount point
        let mount_point = self.work_dir.join(format!("mnt-{}", build_id));
        fs::create_dir_all(&mount_point).await?;

        // Mount the image
        self.mount_image(&image_path, &mount_point).await?;

        // Write files inside mount
        let result = async {
            // Write Dockerfile
            fs::write(mount_point.join("Dockerfile"), dockerfile).await?;

            // Write Kraftfile if present
            if let Some(kraftfile) = kraftfile {
                fs::write(mount_point.join("Kraftfile"), kraftfile).await?;
            }

            // Create code directory
            let code_dir = mount_point.join("code");
            fs::create_dir_all(&code_dir).await?;

            // Extract tarball if it exists
            if code_tarball.exists() {
                self.extract_tarball(code_tarball, &code_dir).await?;
            }

            Ok::<(), BuildError>(())
        }
        .await;

        // Always unmount, even on error
        if let Err(e) = self.unmount(&mount_point).await {
            error!("Failed to unmount {}: {}", mount_point.display(), e);
        }

        // Clean up mount point directory
        let _ = fs::remove_dir(&mount_point).await;

        // Propagate any error from the file operations
        result?;

        info!("Prepared input drive: {}", image_path.display());
        Ok(image_path)
    }

    /// Create an empty output drive for build artifacts.
    pub async fn create_output_drive(
        &self,
        build_id: &str,
        size_mib: u32,
    ) -> Result<PathBuf, BuildError> {
        let image_name = format!("output-{}", build_id);
        self.create_ext4_image(&image_name, size_mib).await
    }

    /// Extract build artifacts from the output drive.
    ///
    /// Looks for:
    /// - `/*.unik` - The built unikernel binary
    /// - `/build.log` - Build output logs
    /// - `/exit_code` - Build exit status
    ///
    /// Returns (unikernel_path, logs).
    pub async fn extract_artifacts(
        &self,
        output_drive: &Path,
        build_id: &str,
        dest_dir: &Path,
    ) -> Result<(PathBuf, String), BuildError> {
        // Create mount point
        let mount_point = self.work_dir.join(format!("mnt-out-{}", build_id));
        fs::create_dir_all(&mount_point).await?;

        // Mount the image (read-only)
        self.mount_image_ro(output_drive, &mount_point).await?;

        let result = async {
            // Check exit code
            let exit_code_path = mount_point.join("exit_code");
            if exit_code_path.exists() {
                let exit_code = fs::read_to_string(&exit_code_path)
                    .await?
                    .trim()
                    .parse::<i32>()
                    .unwrap_or(-1);

                if exit_code != 0 {
                    // Read logs for error context
                    let logs = self.read_logs(&mount_point).await;
                    return Err(BuildError::ArtifactExtractionFailed(format!(
                        "Build failed with exit code {}: {}",
                        exit_code,
                        logs.lines().take(10).collect::<Vec<_>>().join("\n")
                    )));
                }
            }

            // Read logs
            let logs = self.read_logs(&mount_point).await;

            // Find unikernel file
            let unikernel_src = self.find_unikernel(&mount_point).await?;

            // Copy to destination
            fs::create_dir_all(dest_dir).await?;
            let unikernel_dest = dest_dir.join(format!("{}.unik", build_id));
            fs::copy(&unikernel_src, &unikernel_dest).await?;

            Ok((unikernel_dest, logs))
        }
        .await;

        // Always unmount
        if let Err(e) = self.unmount(&mount_point).await {
            error!("Failed to unmount {}: {}", mount_point.display(), e);
        }
        let _ = fs::remove_dir(&mount_point).await;

        result
    }

    /// Calculate cache key from build inputs.
    pub fn calculate_cache_key(
        dockerfile: &str,
        kraftfile: Option<&str>,
        code_tarball: &Path,
    ) -> Result<String, BuildError> {
        let mut hasher = Sha256::new();

        // Hash Dockerfile
        hasher.update(dockerfile.as_bytes());
        hasher.update(b"\n---SEPARATOR---\n");

        // Hash Kraftfile if present
        if let Some(kf) = kraftfile {
            hasher.update(kf.as_bytes());
        }
        hasher.update(b"\n---SEPARATOR---\n");

        // Hash code tarball contents
        if code_tarball.exists() {
            let contents = std::fs::read(code_tarball).map_err(BuildError::IoError)?;
            hasher.update(&contents);
        }

        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    }

    /// Clean up all drive files for a build.
    pub async fn cleanup(&self, build_id: &str) -> Result<(), BuildError> {
        let input_path = self.work_dir.join(format!("input-{}.ext4", build_id));
        let output_path = self.work_dir.join(format!("output-{}.ext4", build_id));

        if input_path.exists() {
            fs::remove_file(&input_path).await?;
        }
        if output_path.exists() {
            fs::remove_file(&output_path).await?;
        }

        debug!("Cleaned up drives for build {}", build_id);
        Ok(())
    }

    // --- Private helpers ---

    async fn mount_image(&self, image: &Path, mount_point: &Path) -> Result<(), BuildError> {
        let output = Command::new("mount")
            .args([
                "-o",
                "loop",
                image.to_str().unwrap(),
                mount_point.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| BuildError::DriveError(format!("mount failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!("mount failed: {}", stderr)));
        }

        Ok(())
    }

    async fn mount_image_ro(&self, image: &Path, mount_point: &Path) -> Result<(), BuildError> {
        let output = Command::new("mount")
            .args([
                "-o",
                "loop,ro",
                image.to_str().unwrap(),
                mount_point.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| BuildError::DriveError(format!("mount failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!("mount failed: {}", stderr)));
        }

        Ok(())
    }

    async fn unmount(&self, mount_point: &Path) -> Result<(), BuildError> {
        let output = Command::new("umount")
            .arg(mount_point.to_str().unwrap())
            .output()
            .map_err(|e| BuildError::DriveError(format!("umount failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!("umount failed: {}", stderr)));
        }

        Ok(())
    }

    async fn extract_tarball(&self, tarball: &Path, dest: &Path) -> Result<(), BuildError> {
        let output = Command::new("tar")
            .args([
                "-xf",
                tarball.to_str().unwrap(),
                "-C",
                dest.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| BuildError::DriveError(format!("tar extraction failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DriveError(format!(
                "tar extraction failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    async fn read_logs(&self, mount_point: &Path) -> String {
        let log_path = mount_point.join("build.log");
        if log_path.exists() {
            fs::read_to_string(&log_path).await.unwrap_or_default()
        } else {
            String::new()
        }
    }

    async fn find_unikernel(&self, mount_point: &Path) -> Result<PathBuf, BuildError> {
        // Look for .unik files
        let mut entries = fs::read_dir(mount_point).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "unik") {
                return Ok(path);
            }
        }

        // Also check common Unikraft output locations
        let kraft_output = mount_point.join(".unikraft/build/kernel");
        if kraft_output.exists() {
            return Ok(kraft_output);
        }

        Err(BuildError::ArtifactExtractionFailed(
            "No unikernel binary found in output drive".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn cache_key_deterministic() {
        let key1 = DriveHelper::calculate_cache_key(
            "FROM alpine",
            Some("name: test"),
            Path::new("/nonexistent"),
        )
        .unwrap();
        let key2 = DriveHelper::calculate_cache_key(
            "FROM alpine",
            Some("name: test"),
            Path::new("/nonexistent"),
        )
        .unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn cache_key_varies_with_dockerfile() {
        let key1 = DriveHelper::calculate_cache_key("FROM alpine", None, Path::new("/nonexistent"))
            .unwrap();
        let key2 = DriveHelper::calculate_cache_key("FROM ubuntu", None, Path::new("/nonexistent"))
            .unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn drive_helper_work_dir() {
        let helper = DriveHelper::new("/tmp/test");
        assert_eq!(helper.work_dir(), Path::new("/tmp/test"));
    }

    #[tokio::test]
    async fn cleanup_nonexistent_is_ok() {
        let helper = DriveHelper::new(temp_dir().join("ephemeral-test"));
        // Should not error on nonexistent files
        helper.cleanup("nonexistent-build").await.unwrap();
    }
}
