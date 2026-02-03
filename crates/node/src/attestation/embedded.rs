//! Build-time embedded attestation values
//!
//! These values are embedded at build time by the Yocto build system.
//! The node binary will only start on a platform that matches these values.
//!
//! # Build Process
//!
//! 1. Yocto builds the OS image without the node binary (or with placeholder)
//! 2. dm-verity root hash is measured
//! 3. TPM PCR values are pre-calculated for the boot chain
//! 4. Node binary is rebuilt with these values as environment variables
//! 5. Final OS image is built with the new node binary
//! 6. Final hash is verified to match embedded value

/// Expected dm-verity root hash (hex-encoded)
///
/// Set via `GRAPHENE_VERITY_ROOT` environment variable at build time.
/// If not set, verification is skipped (development mode).
pub const EXPECTED_VERITY_ROOT: Option<&str> = option_env!("GRAPHENE_VERITY_ROOT");

/// Expected TPM PCR 0 value (hex-encoded)
///
/// PCR 0 measures the BIOS/firmware. Set via `GRAPHENE_PCR_0`.
pub const EXPECTED_PCR_0: Option<&str> = option_env!("GRAPHENE_PCR_0");

/// Expected TPM PCR 7 value (hex-encoded)
///
/// PCR 7 measures the Secure Boot state. Set via `GRAPHENE_PCR_7`.
pub const EXPECTED_PCR_7: Option<&str> = option_env!("GRAPHENE_PCR_7");

/// Platform identifier embedded at build time
///
/// Set via `GRAPHENE_PLATFORM_ID`. Defaults to "graphene-os-dev".
pub const PLATFORM_ID: &str = match option_env!("GRAPHENE_PLATFORM_ID") {
    Some(id) => id,
    None => "graphene-os-dev",
};

/// Build timestamp (Unix epoch)
///
/// Set via `GRAPHENE_BUILD_TIME`. Used for quote freshness validation.
pub const BUILD_TIME: Option<&str> = option_env!("GRAPHENE_BUILD_TIME");

/// Check if we're running in development mode (no embedded values)
pub fn is_development_mode() -> bool {
    EXPECTED_VERITY_ROOT.is_none() && EXPECTED_PCR_0.is_none()
}

/// Get all expected values as a tuple, if available
pub fn get_expected_values() -> Option<(&'static str, &'static str, &'static str)> {
    match (EXPECTED_VERITY_ROOT, EXPECTED_PCR_0, EXPECTED_PCR_7) {
        (Some(verity), Some(pcr0), Some(pcr7)) => Some((verity, pcr0, pcr7)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_development_mode_without_env() {
        // Without build-time env vars, we're in dev mode
        // This will pass in normal test runs
        assert!(is_development_mode() || get_expected_values().is_some());
    }

    #[test]
    fn test_platform_id_default() {
        // Default platform ID when not set
        assert!(!PLATFORM_ID.is_empty());
    }
}
