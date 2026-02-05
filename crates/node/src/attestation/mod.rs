//! Platform attestation for Graphene Node OS
//!
//! This module provides TPM-based attestation and dm-verity verification
//! to ensure the node binary only runs on verified Graphene OS installations.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Attestation Flow                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Node Binary Startup                                         │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │  1. Read embedded expected values (VERITY_ROOT, PCRs)   ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │  2. Verify dm-verity root hash matches expected         ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │  3. Read TPM PCR values and verify against expected     ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │  4. If all pass: continue startup                       ││
//! │  │     If any fail: exit with attestation error            ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod embedded;
pub mod mock;
pub mod tpm;
pub mod types;
pub mod verity;

pub use types::{
    AttestationError, AttestationQuote, PcrValues, PlatformAttestor, PlatformIdentity,
};

#[cfg(target_os = "linux")]
pub use tpm::TpmAttestor;

pub use mock::MockAttestor;
pub use verity::VerityVerifier;

/// Enforce platform attestation before worker startup.
pub async fn enforce_attestation(
    attestor: &dyn PlatformAttestor,
) -> Result<PlatformIdentity, AttestationError> {
    if !attestor.is_supported() {
        return Err(AttestationError::TpmNotAvailable(
            "Attestation not supported on this platform".to_string(),
        ));
    }

    attestor.verify_platform().await
}
