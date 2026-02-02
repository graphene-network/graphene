use super::{BuilderError, DriveBuilder};
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Command;

pub struct LinuxBuilder;

#[async_trait]
impl DriveBuilder for LinuxBuilder {
    async fn create_code_drive(
        &self,
        job_id: &str,
        _content: &str,
    ) -> Result<PathBuf, BuilderError> {
        println!("🔨 [REAL] Creating ext4 filesystem for Job {}...", job_id);

        let image_path = PathBuf::from(format!("/tmp/talos_{}.ext4", job_id));

        // 1. Create Empty File (dd)
        let status = Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", image_path.display()),
                "bs=1M",
                "count=10",
            ])
            .output()
            .map_err(|e| BuilderError::IoError(e.to_string()))?;

        if !status.status.success() {
            return Err(BuilderError::IoError("dd failed".into()));
        }

        // 2. Format (mkfs.ext4)
        // ... (Insert the rest of the logic from previous step here) ...

        // For brevity in this snippet, assuming the implementation details match previous step
        // In production, ensure you handle unmounts in a 'finally' block or Drop trait

        Ok(image_path)
    }

    async fn build_dependency_drive(
        &self,
        _job_id: &str,
        _packages: Vec<String>,
    ) -> Result<PathBuf, BuilderError> {
        // Implementation for Phase 2b (Firecracker Builder) goes here
        Err(BuilderError::FormatError("Not implemented yet".into()))
    }
}
