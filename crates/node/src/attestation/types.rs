//! Attestation types and trait definitions

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Errors that can occur during platform attestation
#[derive(Debug)]
pub enum AttestationError {
    /// TPM device not available
    TpmNotAvailable(String),
    /// dm-verity not configured or failed
    VerityNotConfigured(String),
    /// PCR values don't match expected
    PcrMismatch {
        pcr: u8,
        expected: String,
        actual: String,
    },
    /// dm-verity root hash doesn't match expected
    VerityRootMismatch { expected: String, actual: String },
    /// TPM communication error
    TpmError(String),
    /// General I/O error
    IoError(std::io::Error),
    /// Platform not verified (general failure)
    PlatformNotVerified(String),
    /// Expected values not embedded in binary
    MissingEmbeddedValues(String),
}

impl std::error::Error for AttestationError {}

impl Display for AttestationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AttestationError::TpmNotAvailable(msg) => {
                write!(f, "TPM device not available: {}", msg)
            }
            AttestationError::VerityNotConfigured(msg) => {
                write!(f, "dm-verity not configured: {}", msg)
            }
            AttestationError::PcrMismatch {
                pcr,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "PCR {} mismatch: expected {}, got {}",
                    pcr, expected, actual
                )
            }
            AttestationError::VerityRootMismatch { expected, actual } => {
                write!(
                    f,
                    "dm-verity root hash mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            AttestationError::TpmError(msg) => write!(f, "TPM error: {}", msg),
            AttestationError::IoError(err) => write!(f, "I/O error: {}", err),
            AttestationError::PlatformNotVerified(msg) => {
                write!(f, "Platform verification failed: {}", msg)
            }
            AttestationError::MissingEmbeddedValues(msg) => {
                write!(f, "Missing embedded attestation values: {}", msg)
            }
        }
    }
}

impl From<std::io::Error> for AttestationError {
    fn from(err: std::io::Error) -> Self {
        AttestationError::IoError(err)
    }
}

/// TPM Platform Configuration Register values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PcrValues {
    /// PCR 0: BIOS/firmware measurements
    pub pcr_0: String,
    /// PCR 7: Secure Boot state
    pub pcr_7: String,
    /// Optional additional PCRs
    #[serde(default)]
    pub additional: std::collections::HashMap<u8, String>,
}

impl PcrValues {
    /// Create new PCR values from strings (hex-encoded)
    pub fn new(pcr_0: impl Into<String>, pcr_7: impl Into<String>) -> Self {
        Self {
            pcr_0: pcr_0.into(),
            pcr_7: pcr_7.into(),
            additional: std::collections::HashMap::new(),
        }
    }

    /// Add an additional PCR value
    pub fn with_pcr(mut self, pcr_index: u8, value: impl Into<String>) -> Self {
        self.additional.insert(pcr_index, value.into());
        self
    }
}

/// TPM attestation quote for network registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationQuote {
    /// TPM-signed quote data
    pub quote: Vec<u8>,
    /// Signature over the quote
    pub signature: Vec<u8>,
    /// PCR values included in quote
    pub pcr_values: PcrValues,
    /// Nonce used for freshness
    pub nonce: Vec<u8>,
    /// Attestation key public portion (for verification)
    pub ak_public: Vec<u8>,
}

/// Platform identity after successful verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformIdentity {
    /// Verified dm-verity root hash
    pub verity_root: String,
    /// Verified PCR values
    pub pcr_values: PcrValues,
    /// Timestamp of verification
    pub verified_at: u64,
    /// Platform identifier (e.g., "graphene-os-v0.1.0")
    pub platform_id: String,
}

impl Display for PlatformIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}[verity:{}..., pcr0:{}...]",
            self.platform_id,
            &self.verity_root[..8.min(self.verity_root.len())],
            &self.pcr_values.pcr_0[..8.min(self.pcr_values.pcr_0.len())]
        )
    }
}

/// Platform attestation trait
///
/// Implementations verify that the node binary is running on a trusted
/// Graphene OS installation and can generate quotes for network registration.
#[async_trait]
pub trait PlatformAttestor: Send + Sync {
    /// Verify we're running on a trusted Graphene OS
    ///
    /// This checks:
    /// 1. dm-verity root hash matches embedded expected value
    /// 2. TPM PCR values match embedded expected values
    ///
    /// Returns platform identity on success, error if verification fails.
    async fn verify_platform(&self) -> Result<PlatformIdentity, AttestationError>;

    /// Generate attestation quote for network registration
    ///
    /// The quote is TPM-signed and includes:
    /// - Current PCR values
    /// - The provided nonce (for freshness)
    /// - Signature from the attestation key
    async fn generate_quote(&self, nonce: &[u8]) -> Result<AttestationQuote, AttestationError>;

    /// Check if attestation is supported on this platform
    fn is_supported(&self) -> bool;
}
