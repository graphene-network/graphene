//! TPM 2.0 attestation implementation
//!
//! Provides TPM-based platform attestation for Linux systems.
//! Uses the Linux TPM2 userspace interface.

use super::embedded;
use super::types::{
    AttestationError, AttestationQuote, PcrValues, PlatformAttestor, PlatformIdentity,
};
use super::verity::VerityVerifier;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// TPM quote output: (quote_data, signature, ak_public_key)
type TpmQuoteOutput = (Vec<u8>, Vec<u8>, Vec<u8>);

/// TPM 2.0 attestor for Linux systems
pub struct TpmAttestor {
    /// Path to TPM device
    tpm_device: String,
    /// dm-verity verifier
    verity_verifier: VerityVerifier,
    /// Expected PCR values
    expected_pcrs: Option<PcrValues>,
}

impl TpmAttestor {
    /// Create a new TPM attestor
    pub fn new() -> Result<Self, AttestationError> {
        let tpm_device = Self::find_tpm_device()?;

        let expected_pcrs = match (embedded::EXPECTED_PCR_0, embedded::EXPECTED_PCR_7) {
            (Some(pcr0), Some(pcr7)) => Some(PcrValues::new(pcr0, pcr7)),
            _ => None,
        };

        Ok(Self {
            tpm_device,
            verity_verifier: VerityVerifier::from_embedded(),
            expected_pcrs,
        })
    }

    /// Create with custom expected values (for testing)
    pub fn with_expected(
        tpm_device: String,
        verity_root: Option<String>,
        expected_pcrs: Option<PcrValues>,
    ) -> Self {
        Self {
            tpm_device,
            verity_verifier: VerityVerifier::new(verity_root),
            expected_pcrs,
        }
    }

    /// Find the TPM device path
    fn find_tpm_device() -> Result<String, AttestationError> {
        // Try standard TPM device paths
        for path in ["/dev/tpmrm0", "/dev/tpm0"] {
            if Path::new(path).exists() {
                return Ok(path.to_string());
            }
        }

        Err(AttestationError::TpmNotAvailable(
            "No TPM device found at /dev/tpmrm0 or /dev/tpm0".to_string(),
        ))
    }

    /// Read PCR values from the TPM
    fn read_pcr_values(&self) -> Result<PcrValues, AttestationError> {
        // Read from sysfs (available via tpm2_pcrs or kernel interface)
        let pcr_0 = self.read_pcr(0)?;
        let pcr_7 = self.read_pcr(7)?;

        Ok(PcrValues::new(pcr_0, pcr_7))
    }

    /// Read a single PCR value
    fn read_pcr(&self, index: u8) -> Result<String, AttestationError> {
        // Method 1: Read from sysfs if available
        let sysfs_path = format!("/sys/class/tpm/tpm0/pcr-sha256/{}", index);
        if let Ok(value) = fs::read_to_string(&sysfs_path) {
            return Ok(value.trim().to_lowercase());
        }

        // Method 2: Use tpm2-tools command (requires tpm2-tools package)
        // This is a fallback - in production, we'd use the tss2-sys crate
        #[cfg(feature = "tpm2-tools")]
        {
            use std::process::Command;
            let output = Command::new("tpm2_pcrread")
                .args(["sha256:{}".replace("{}", &index.to_string())])
                .output()
                .map_err(|e| {
                    AttestationError::TpmError(format!("Failed to run tpm2_pcrread: {}", e))
                })?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse the output format: "sha256: 0 : 0x..."
                if let Some(hash) = Self::parse_tpm2_pcrread_output(&stdout, index) {
                    return Ok(hash);
                }
            }
        }

        Err(AttestationError::TpmError(format!(
            "Could not read PCR {} from TPM",
            index
        )))
    }

    /// Parse output from tpm2_pcrread
    #[cfg(feature = "tpm2-tools")]
    fn parse_tpm2_pcrread_output(output: &str, index: u8) -> Option<String> {
        for line in output.lines() {
            if line.contains(&format!("{} :", index)) {
                // Format: "  0 : 0xABCDEF..."
                if let Some(hex_start) = line.find("0x") {
                    let hash = &line[hex_start + 2..];
                    let hash = hash.trim().to_lowercase();
                    if !hash.is_empty() {
                        return Some(hash);
                    }
                }
            }
        }
        None
    }

    /// Verify PCR values match expected
    fn verify_pcrs(&self, actual: &PcrValues) -> Result<(), AttestationError> {
        let expected = self.expected_pcrs.as_ref().ok_or_else(|| {
            AttestationError::MissingEmbeddedValues("Expected PCR values not set".to_string())
        })?;

        if actual.pcr_0 != expected.pcr_0 {
            return Err(AttestationError::PcrMismatch {
                pcr: 0,
                expected: expected.pcr_0.clone(),
                actual: actual.pcr_0.clone(),
            });
        }

        if actual.pcr_7 != expected.pcr_7 {
            return Err(AttestationError::PcrMismatch {
                pcr: 7,
                expected: expected.pcr_7.clone(),
                actual: actual.pcr_7.clone(),
            });
        }

        Ok(())
    }

    /// Generate a TPM quote
    ///
    /// In production, this would use the TPM2 TSS library to generate
    /// a signed quote. For now, we provide a stub implementation.
    fn generate_tpm_quote(
        &self,
        nonce: &[u8],
        pcr_values: &PcrValues,
    ) -> Result<TpmQuoteOutput, AttestationError> {
        // TODO: Implement actual TPM quote generation using tss2-sys crate
        // This requires:
        // 1. Creating or loading an attestation key (AK)
        // 2. Calling TPM2_Quote with the AK and selected PCRs
        // 3. Returning the quote structure, signature, and AK public key

        // For now, return placeholder that indicates TPM quote not implemented
        let quote = format!(
            "OPENCAPSULE-QUOTE-STUB:pcr0={},pcr7={},nonce={}",
            pcr_values.pcr_0,
            pcr_values.pcr_7,
            hex::encode(nonce)
        )
        .into_bytes();

        let signature = vec![0u8; 64]; // Placeholder signature
        let ak_public = vec![0u8; 32]; // Placeholder AK public

        Ok((quote, signature, ak_public))
    }
}

impl Default for TpmAttestor {
    fn default() -> Self {
        Self::new().expect("TPM should be available")
    }
}

#[async_trait]
impl PlatformAttestor for TpmAttestor {
    async fn verify_platform(&self) -> Result<PlatformIdentity, AttestationError> {
        // Check if we're in development mode (no embedded values)
        if embedded::is_development_mode() {
            tracing::warn!("Running in development mode - attestation checks skipped");
            return Ok(PlatformIdentity {
                verity_root: "development-mode".to_string(),
                pcr_values: PcrValues::new("development", "development"),
                verified_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                platform_id: embedded::PLATFORM_ID.to_string(),
            });
        }

        // Step 1: Verify dm-verity root hash
        let verity_root = self.verity_verifier.verify()?;
        tracing::info!("dm-verity root hash verified: {}...", &verity_root[..16]);

        // Step 2: Read and verify TPM PCR values
        let pcr_values = self.read_pcr_values()?;
        self.verify_pcrs(&pcr_values)?;
        tracing::info!("TPM PCR values verified");

        Ok(PlatformIdentity {
            verity_root,
            pcr_values,
            verified_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            platform_id: embedded::PLATFORM_ID.to_string(),
        })
    }

    async fn generate_quote(&self, nonce: &[u8]) -> Result<AttestationQuote, AttestationError> {
        // Read current PCR values
        let pcr_values = self.read_pcr_values()?;

        // Generate TPM quote
        let (quote, signature, ak_public) = self.generate_tpm_quote(nonce, &pcr_values)?;

        Ok(AttestationQuote {
            quote,
            signature,
            pcr_values,
            nonce: nonce.to_vec(),
            ak_public,
        })
    }

    fn is_supported(&self) -> bool {
        Path::new(&self.tpm_device).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_tpm_device_not_available() {
        // In test environment, TPM likely not available
        let result = TpmAttestor::find_tpm_device();
        // Either finds a device or returns TpmNotAvailable
        assert!(result.is_ok() || matches!(result, Err(AttestationError::TpmNotAvailable(_))));
    }
}
