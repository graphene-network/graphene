//! Cryptographic primitives for encrypted job I/O.
//!
//! This module provides end-to-end encryption for job inputs and outputs,
//! binding encryption keys to payment channel relationships for "soft
//! confidential computing" without requiring hardware TEEs.
//!
//! # Key Derivation
//!
//! Two-layer key derivation provides both payment binding and forward secrecy:
//!
//! 1. **Channel Master Key** (long-lived): Derived via ECDH between user and
//!    worker X25519 keys, with the Solana channel PDA as salt. This binds
//!    encryption to the payment relationship.
//!
//! 2. **Per-Job Key** (ephemeral): Derived from an ephemeral X25519 keypair
//!    combined with the channel key. Each job uses a unique key, providing
//!    forward secrecy.
//!
//! # Encryption
//!
//! Uses XChaCha20-Poly1305 with 192-bit random nonces. The large nonce space
//! eliminates collision risk when using random nonces.

mod channel_keys;
mod job_crypto;
mod mock;

pub use channel_keys::{ChannelKeys, Ed25519ToX25519Error};
pub use job_crypto::{
    decrypt_blob, encrypt_blob, EncryptedBlob, JobCryptoError, ENCRYPTED_BLOB_VERSION,
};
pub use mock::MockCryptoProvider;

use async_trait::async_trait;

/// Errors that can occur during cryptographic operations.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Key conversion failed: {0}")]
    KeyConversion(#[from] Ed25519ToX25519Error),

    #[error("Encryption/decryption failed: {0}")]
    JobCrypto(#[from] JobCryptoError),

    #[error("Invalid key length: expected {expected}, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },

    #[error("Missing channel keys for peer")]
    MissingChannelKeys,
}

/// Trait for cryptographic operations on job data.
///
/// Implementations handle key derivation, encryption, and decryption of job
/// inputs and outputs. The trait allows for mock implementations in tests.
#[async_trait]
pub trait CryptoProvider: Send + Sync {
    /// Derive channel keys for a peer using the local secret key.
    ///
    /// # Arguments
    /// * `local_ed25519_secret` - Our Ed25519 secret key (32 bytes)
    /// * `peer_ed25519_public` - Peer's Ed25519 public key (32 bytes)
    /// * `channel_pda` - Solana PDA for the payment channel (salt)
    fn derive_channel_keys(
        &self,
        local_ed25519_secret: &[u8; 32],
        peer_ed25519_public: &[u8; 32],
        channel_pda: &[u8; 32],
    ) -> Result<ChannelKeys, CryptoError>;

    /// Encrypt a blob for a specific job.
    ///
    /// # Arguments
    /// * `plaintext` - Data to encrypt
    /// * `channel_keys` - Pre-derived channel keys
    /// * `job_id` - Unique job identifier (used in key derivation)
    /// * `direction` - Whether this is input or output encryption
    ///
    /// # Returns
    /// Encrypted blob containing version, ephemeral pubkey, nonce, ciphertext, and tag.
    fn encrypt_job_blob(
        &self,
        plaintext: &[u8],
        channel_keys: &ChannelKeys,
        job_id: &str,
        direction: EncryptionDirection,
    ) -> Result<EncryptedBlob, CryptoError>;

    /// Decrypt a blob for a specific job.
    ///
    /// # Arguments
    /// * `encrypted` - The encrypted blob
    /// * `channel_keys` - Pre-derived channel keys
    /// * `job_id` - Unique job identifier (must match encryption)
    /// * `direction` - Whether this is input or output decryption
    ///
    /// # Returns
    /// Decrypted plaintext.
    fn decrypt_job_blob(
        &self,
        encrypted: &EncryptedBlob,
        channel_keys: &ChannelKeys,
        job_id: &str,
        direction: EncryptionDirection,
    ) -> Result<Vec<u8>, CryptoError>;
}

/// Direction of encryption (affects HKDF info string for domain separation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionDirection {
    /// User encrypting input/code for worker
    Input,
    /// Worker encrypting result for user
    Output,
}

impl EncryptionDirection {
    /// Get the HKDF info string for this direction.
    pub fn hkdf_info(&self) -> &'static [u8] {
        match self {
            Self::Input => b"graphene-job-input-v1",
            Self::Output => b"graphene-job-output-v1",
        }
    }
}

/// Default implementation of CryptoProvider using real cryptographic primitives.
#[derive(Debug, Default)]
pub struct DefaultCryptoProvider;

#[async_trait]
impl CryptoProvider for DefaultCryptoProvider {
    fn derive_channel_keys(
        &self,
        local_ed25519_secret: &[u8; 32],
        peer_ed25519_public: &[u8; 32],
        channel_pda: &[u8; 32],
    ) -> Result<ChannelKeys, CryptoError> {
        Ok(ChannelKeys::derive(
            local_ed25519_secret,
            peer_ed25519_public,
            channel_pda,
        )?)
    }

    fn encrypt_job_blob(
        &self,
        plaintext: &[u8],
        channel_keys: &ChannelKeys,
        job_id: &str,
        direction: EncryptionDirection,
    ) -> Result<EncryptedBlob, CryptoError> {
        Ok(encrypt_blob(plaintext, channel_keys, job_id, direction)?)
    }

    fn decrypt_job_blob(
        &self,
        encrypted: &EncryptedBlob,
        channel_keys: &ChannelKeys,
        job_id: &str,
        direction: EncryptionDirection,
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(decrypt_blob(encrypted, channel_keys, job_id, direction)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_encryption() {
        let provider = DefaultCryptoProvider;

        // Generate test keys (in real usage these come from Iroh identity)
        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];

        // Derive public keys from secrets for the test
        let user_signing_key = ed25519_dalek::SigningKey::from_bytes(&user_secret);
        let worker_signing_key = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
        let user_public: [u8; 32] = user_signing_key.verifying_key().to_bytes();
        let worker_public: [u8; 32] = worker_signing_key.verifying_key().to_bytes();

        let channel_pda = [3u8; 32];
        let job_id = "test-job-123";
        let plaintext = b"Hello, confidential world!";

        // User derives channel keys and encrypts
        let user_channel_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_pda)
            .expect("channel key derivation failed");

        let encrypted = provider
            .encrypt_job_blob(
                plaintext,
                &user_channel_keys,
                job_id,
                EncryptionDirection::Input,
            )
            .expect("encryption failed");

        // Worker derives channel keys and decrypts
        let worker_channel_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_pda)
            .expect("channel key derivation failed");

        let decrypted = provider
            .decrypt_job_blob(
                &encrypted,
                &worker_channel_keys,
                job_id,
                EncryptionDirection::Input,
            )
            .expect("decryption failed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_job_id_fails() {
        let provider = DefaultCryptoProvider;

        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];
        let user_signing_key = ed25519_dalek::SigningKey::from_bytes(&user_secret);
        let worker_signing_key = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
        let user_public: [u8; 32] = user_signing_key.verifying_key().to_bytes();
        let worker_public: [u8; 32] = worker_signing_key.verifying_key().to_bytes();
        let channel_pda = [3u8; 32];

        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_pda)
            .unwrap();
        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_pda)
            .unwrap();

        let encrypted = provider
            .encrypt_job_blob(
                b"secret data",
                &user_keys,
                "job-1",
                EncryptionDirection::Input,
            )
            .unwrap();

        // Try to decrypt with wrong job ID
        let result = provider.decrypt_job_blob(
            &encrypted,
            &worker_keys,
            "job-2", // Wrong job ID
            EncryptionDirection::Input,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_direction_separation() {
        let provider = DefaultCryptoProvider;

        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];
        let user_signing_key = ed25519_dalek::SigningKey::from_bytes(&user_secret);
        let worker_signing_key = ed25519_dalek::SigningKey::from_bytes(&worker_secret);
        let user_public: [u8; 32] = user_signing_key.verifying_key().to_bytes();
        let worker_public: [u8; 32] = worker_signing_key.verifying_key().to_bytes();
        let channel_pda = [3u8; 32];

        let user_keys = provider
            .derive_channel_keys(&user_secret, &worker_public, &channel_pda)
            .unwrap();
        let worker_keys = provider
            .derive_channel_keys(&worker_secret, &user_public, &channel_pda)
            .unwrap();

        // Encrypt as input
        let encrypted = provider
            .encrypt_job_blob(
                b"secret data",
                &user_keys,
                "job-1",
                EncryptionDirection::Input,
            )
            .unwrap();

        // Try to decrypt as output (wrong direction)
        let result = provider.decrypt_job_blob(
            &encrypted,
            &worker_keys,
            "job-1",
            EncryptionDirection::Output, // Wrong direction
        );

        assert!(result.is_err());
    }
}
