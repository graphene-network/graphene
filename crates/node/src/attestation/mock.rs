//! Mock attestor for testing
//!
//! Provides configurable attestation behavior for unit tests.

use super::types::{
    AttestationError, AttestationQuote, PcrValues, PlatformAttestor, PlatformIdentity,
};
use async_trait::async_trait;
use std::time::{SystemTime, UNIX_EPOCH};

/// Mock behavior for testing
#[derive(Clone, Debug)]
pub enum MockBehavior {
    /// All verifications pass
    HappyPath,
    /// dm-verity verification fails
    VerityMismatch,
    /// TPM PCR verification fails
    PcrMismatch { pcr: u8 },
    /// TPM not available
    TpmUnavailable,
    /// Quote generation fails
    QuoteFails,
}

/// Mock attestor for testing
pub struct MockAttestor {
    behavior: MockBehavior,
    verity_root: String,
    pcr_values: PcrValues,
}

impl MockAttestor {
    /// Create a new mock attestor with specified behavior
    pub fn new(behavior: MockBehavior) -> Self {
        Self {
            behavior,
            verity_root: "mock-verity-root-hash-abc123def456".to_string(),
            pcr_values: PcrValues::new(
                "mock-pcr0-0000000000000000000000000000000000000000000000000000000000000000",
                "mock-pcr7-1111111111111111111111111111111111111111111111111111111111111111",
            ),
        }
    }

    /// Create a happy-path mock (all checks pass)
    pub fn happy_path() -> Self {
        Self::new(MockBehavior::HappyPath)
    }

    /// Set custom verity root for testing
    pub fn with_verity_root(mut self, root: impl Into<String>) -> Self {
        self.verity_root = root.into();
        self
    }

    /// Set custom PCR values for testing
    pub fn with_pcr_values(mut self, pcr_values: PcrValues) -> Self {
        self.pcr_values = pcr_values;
        self
    }
}

#[async_trait]
impl PlatformAttestor for MockAttestor {
    async fn verify_platform(&self) -> Result<PlatformIdentity, AttestationError> {
        match &self.behavior {
            MockBehavior::HappyPath => {
                tracing::info!("[MOCK] Platform verification passed");
                Ok(PlatformIdentity {
                    verity_root: self.verity_root.clone(),
                    pcr_values: self.pcr_values.clone(),
                    verified_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    platform_id: "mock-graphene-os-v1.0.0".to_string(),
                })
            }
            MockBehavior::VerityMismatch => {
                tracing::warn!("[MOCK] Simulating dm-verity mismatch");
                Err(AttestationError::VerityRootMismatch {
                    expected: "expected-hash".to_string(),
                    actual: "actual-hash-mismatch".to_string(),
                })
            }
            MockBehavior::PcrMismatch { pcr } => {
                tracing::warn!("[MOCK] Simulating PCR {} mismatch", pcr);
                Err(AttestationError::PcrMismatch {
                    pcr: *pcr,
                    expected: "expected-pcr-value".to_string(),
                    actual: "actual-pcr-mismatch".to_string(),
                })
            }
            MockBehavior::TpmUnavailable => {
                tracing::warn!("[MOCK] Simulating TPM unavailable");
                Err(AttestationError::TpmNotAvailable(
                    "Mock: TPM device not available".to_string(),
                ))
            }
            MockBehavior::QuoteFails => {
                // Verification passes, but quote will fail
                Ok(PlatformIdentity {
                    verity_root: self.verity_root.clone(),
                    pcr_values: self.pcr_values.clone(),
                    verified_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    platform_id: "mock-graphene-os-v1.0.0".to_string(),
                })
            }
        }
    }

    async fn generate_quote(&self, nonce: &[u8]) -> Result<AttestationQuote, AttestationError> {
        if matches!(self.behavior, MockBehavior::QuoteFails) {
            tracing::warn!("[MOCK] Simulating quote generation failure");
            return Err(AttestationError::TpmError(
                "Mock: Quote generation failed".to_string(),
            ));
        }

        tracing::info!("[MOCK] Generating mock attestation quote");
        Ok(AttestationQuote {
            quote: b"MOCK-QUOTE-DATA".to_vec(),
            signature: b"MOCK-SIGNATURE".to_vec(),
            pcr_values: self.pcr_values.clone(),
            nonce: nonce.to_vec(),
            ak_public: b"MOCK-AK-PUBLIC".to_vec(),
        })
    }

    fn is_supported(&self) -> bool {
        !matches!(self.behavior, MockBehavior::TpmUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_happy_path() {
        let attestor = MockAttestor::happy_path();
        let result = attestor.verify_platform().await;
        assert!(result.is_ok());

        let identity = result.unwrap();
        assert!(identity.platform_id.contains("mock"));
    }

    #[tokio::test]
    async fn test_verity_mismatch() {
        let attestor = MockAttestor::new(MockBehavior::VerityMismatch);
        let result = attestor.verify_platform().await;
        assert!(matches!(
            result,
            Err(AttestationError::VerityRootMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn test_pcr_mismatch() {
        let attestor = MockAttestor::new(MockBehavior::PcrMismatch { pcr: 7 });
        let result = attestor.verify_platform().await;
        assert!(matches!(
            result,
            Err(AttestationError::PcrMismatch { pcr: 7, .. })
        ));
    }

    #[tokio::test]
    async fn test_tpm_unavailable() {
        let attestor = MockAttestor::new(MockBehavior::TpmUnavailable);
        assert!(!attestor.is_supported());

        let result = attestor.verify_platform().await;
        assert!(matches!(result, Err(AttestationError::TpmNotAvailable(_))));
    }

    #[tokio::test]
    async fn test_quote_generation() {
        let attestor = MockAttestor::happy_path();
        let nonce = b"test-nonce-12345";
        let result = attestor.generate_quote(nonce).await;
        assert!(result.is_ok());

        let quote = result.unwrap();
        assert_eq!(quote.nonce, nonce.to_vec());
    }

    #[tokio::test]
    async fn test_quote_failure() {
        let attestor = MockAttestor::new(MockBehavior::QuoteFails);

        // Verification passes
        let verify_result = attestor.verify_platform().await;
        assert!(verify_result.is_ok());

        // But quote fails
        let quote_result = attestor.generate_quote(b"nonce").await;
        assert!(matches!(quote_result, Err(AttestationError::TpmError(_))));
    }
}
