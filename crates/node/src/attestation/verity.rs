//! dm-verity root hash verification
//!
//! Reads the dm-verity root hash from the running system and compares
//! it against the expected value embedded at build time.

use super::types::AttestationError;
use std::fs;
use std::path::Path;

/// dm-verity verifier
pub struct VerityVerifier {
    /// Expected root hash (hex-encoded)
    expected_root: Option<String>,
}

impl VerityVerifier {
    /// Create a new verity verifier with expected root hash
    pub fn new(expected_root: Option<String>) -> Self {
        Self { expected_root }
    }

    /// Create from embedded values
    pub fn from_embedded() -> Self {
        Self::new(super::embedded::EXPECTED_VERITY_ROOT.map(String::from))
    }

    /// Verify dm-verity root hash matches expected
    ///
    /// Returns the actual root hash on success.
    pub fn verify(&self) -> Result<String, AttestationError> {
        let expected = self.expected_root.as_ref().ok_or_else(|| {
            AttestationError::MissingEmbeddedValues(
                "GRAPHENE_VERITY_ROOT not set at build time".to_string(),
            )
        })?;

        let actual = self.read_root_hash()?;

        if actual != *expected {
            return Err(AttestationError::VerityRootMismatch {
                expected: expected.clone(),
                actual,
            });
        }

        Ok(actual)
    }

    /// Check if dm-verity is configured on this system
    pub fn is_configured(&self) -> bool {
        // Check for dm-verity device mapper target
        Path::new("/sys/module/dm_verity").exists() || self.find_verity_device().is_ok()
    }

    /// Read the dm-verity root hash from the system
    fn read_root_hash(&self) -> Result<String, AttestationError> {
        // Method 1: Read from kernel command line
        if let Ok(cmdline) = fs::read_to_string("/proc/cmdline") {
            if let Some(hash) = Self::parse_verity_root_from_cmdline(&cmdline) {
                return Ok(hash);
            }
        }

        // Method 2: Read from dm-verity status via dmsetup
        if let Ok(hash) = self.read_from_dmsetup() {
            return Ok(hash);
        }

        // Method 3: Read from sysfs if available
        if let Ok(hash) = self.read_from_sysfs() {
            return Ok(hash);
        }

        Err(AttestationError::VerityNotConfigured(
            "Could not read dm-verity root hash from system".to_string(),
        ))
    }

    /// Parse root_hash from kernel command line
    ///
    /// Format: `root=/dev/dm-0 dm-mod.create="vroot,,ro,0 ... root_hash=<hash>"`
    fn parse_verity_root_from_cmdline(cmdline: &str) -> Option<String> {
        // Look for verity root hash in various formats
        for pattern in ["root_hash=", "dm-verity.root_hash=", "verity.root_hash="] {
            if let Some(start) = cmdline.find(pattern) {
                let hash_start = start + pattern.len();
                let hash_end = cmdline[hash_start..]
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .map(|i| hash_start + i)
                    .unwrap_or(cmdline.len());

                let hash = &cmdline[hash_start..hash_end];
                if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hash.to_lowercase());
                }
            }
        }
        None
    }

    /// Find the dm-verity device
    fn find_verity_device(&self) -> Result<String, AttestationError> {
        // Look for verity targets in /sys/block/dm-*/dm/name
        let dm_path = Path::new("/sys/block");
        if !dm_path.exists() {
            return Err(AttestationError::VerityNotConfigured(
                "/sys/block not available".to_string(),
            ));
        }

        for entry in fs::read_dir(dm_path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("dm-") {
                let dm_name_path = entry.path().join("dm/name");
                if let Ok(dm_name) = fs::read_to_string(&dm_name_path) {
                    let dm_name = dm_name.trim();
                    if dm_name.contains("verity") || dm_name == "vroot" {
                        return Ok(name_str.to_string());
                    }
                }
            }
        }

        Err(AttestationError::VerityNotConfigured(
            "No dm-verity device found".to_string(),
        ))
    }

    /// Read root hash from dmsetup status
    fn read_from_dmsetup(&self) -> Result<String, AttestationError> {
        // This would require running dmsetup and parsing output
        // For now, return error - can be implemented with tokio::process
        Err(AttestationError::VerityNotConfigured(
            "dmsetup reading not implemented".to_string(),
        ))
    }

    /// Read root hash from sysfs
    fn read_from_sysfs(&self) -> Result<String, AttestationError> {
        // Try to find root hash in dm-verity sysfs entries
        let device = self.find_verity_device()?;
        let hash_path = format!("/sys/block/{}/dm/verity_root_hash", device);

        if Path::new(&hash_path).exists() {
            let hash = fs::read_to_string(&hash_path)?;
            return Ok(hash.trim().to_lowercase());
        }

        Err(AttestationError::VerityNotConfigured(
            "verity_root_hash not in sysfs".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cmdline_root_hash() {
        let cmdline = r#"root=/dev/dm-0 dm-mod.create="vroot,,,ro,0 65536 verity 1 /dev/sda2 /dev/sda3 4096 4096 8192 1 sha256 root_hash=abc123def456""#;
        let hash = VerityVerifier::parse_verity_root_from_cmdline(cmdline);
        assert_eq!(hash, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_parse_cmdline_dm_verity_format() {
        let cmdline = "quiet dm-verity.root_hash=deadbeef1234 splash";
        let hash = VerityVerifier::parse_verity_root_from_cmdline(cmdline);
        assert_eq!(hash, Some("deadbeef1234".to_string()));
    }

    #[test]
    fn test_parse_cmdline_no_hash() {
        let cmdline = "quiet splash";
        let hash = VerityVerifier::parse_verity_root_from_cmdline(cmdline);
        assert_eq!(hash, None);
    }

    #[test]
    fn test_verifier_missing_expected() {
        let verifier = VerityVerifier::new(None);
        let result = verifier.verify();
        assert!(matches!(
            result,
            Err(AttestationError::MissingEmbeddedValues(_))
        ));
    }
}
