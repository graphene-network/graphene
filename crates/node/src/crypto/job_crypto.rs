//! Job-level encryption and decryption using XChaCha20-Poly1305.
//!
//! Provides forward secrecy through per-job ephemeral X25519 keypairs.

use super::{ChannelKeys, EncryptionDirection};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

/// Current version of the encrypted blob format.
pub const ENCRYPTED_BLOB_VERSION: u8 = 1;

/// Errors during job encryption/decryption.
#[derive(Debug, thiserror::Error)]
pub enum JobCryptoError {
    #[error("Unsupported blob version: {0}")]
    UnsupportedVersion(u8),

    #[error("Decryption failed (authentication tag mismatch)")]
    DecryptionFailed,

    #[error("Invalid blob format: {0}")]
    InvalidFormat(String),

    #[error("HKDF expansion failed")]
    HkdfError,
}

/// An encrypted blob with all data needed for decryption.
///
/// Format: `[version: 1][ephemeral_pubkey: 32][nonce: 24][ciphertext+tag: N+16]`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// Format version (currently 1).
    pub version: u8,

    /// Ephemeral X25519 public key used for this encryption.
    pub ephemeral_pubkey: [u8; 32],

    /// 192-bit nonce for XChaCha20-Poly1305.
    pub nonce: [u8; 24],

    /// Ciphertext with appended Poly1305 authentication tag.
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    /// Serialize to bytes for transmission/storage.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + 32 + 24 + self.ciphertext.len());
        bytes.push(self.version);
        bytes.extend_from_slice(&self.ephemeral_pubkey);
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, JobCryptoError> {
        // Minimum size: version(1) + pubkey(32) + nonce(24) + tag(16)
        if bytes.len() < 1 + 32 + 24 + 16 {
            return Err(JobCryptoError::InvalidFormat("Blob too short".to_string()));
        }

        let version = bytes[0];
        if version != ENCRYPTED_BLOB_VERSION {
            return Err(JobCryptoError::UnsupportedVersion(version));
        }

        let mut ephemeral_pubkey = [0u8; 32];
        ephemeral_pubkey.copy_from_slice(&bytes[1..33]);

        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&bytes[33..57]);

        let ciphertext = bytes[57..].to_vec();

        Ok(Self {
            version,
            ephemeral_pubkey,
            nonce,
            ciphertext,
        })
    }
}

/// Encrypt plaintext for a job.
///
/// # Key Derivation (Two Layers)
///
/// 1. Generate ephemeral X25519 keypair
/// 2. ECDH(ephemeral_secret, peer_static_public) → ephemeral_shared
/// 3. Combined = ephemeral_shared || channel_master_key
/// 4. HKDF(combined, salt=job_id, info=direction) → job_key
///
/// This provides forward secrecy (compromised channel keys don't reveal past jobs)
/// while maintaining payment binding (need valid channel relationship).
pub fn encrypt_blob(
    plaintext: &[u8],
    channel_keys: &ChannelKeys,
    job_id: &str,
    direction: EncryptionDirection,
) -> Result<EncryptedBlob, JobCryptoError> {
    // Generate ephemeral keypair for forward secrecy
    let mut ephemeral_secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut ephemeral_secret_bytes);
    let ephemeral_secret = X25519StaticSecret::from(ephemeral_secret_bytes);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    // ECDH with peer's static public key
    let ephemeral_shared = ephemeral_secret.diffie_hellman(channel_keys.peer_x25519_public());

    // Derive job key: HKDF(ephemeral_shared || channel_master, salt=job_id, info=direction)
    let job_key = derive_job_key(
        ephemeral_shared.as_bytes(),
        channel_keys.master_key(),
        job_id,
        direction,
    )?;

    // Generate random nonce (192 bits is safe for random nonces)
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);

    // Encrypt with XChaCha20-Poly1305
    let cipher = XChaCha20Poly1305::new_from_slice(&job_key)
        .expect("32-byte key is valid for XChaCha20-Poly1305");

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| JobCryptoError::DecryptionFailed)?;

    Ok(EncryptedBlob {
        version: ENCRYPTED_BLOB_VERSION,
        ephemeral_pubkey: *ephemeral_public.as_bytes(),
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Decrypt an encrypted blob.
///
/// Reconstructs the job key using:
/// - Our static X25519 secret (from channel_keys)
/// - Sender's ephemeral public key (from blob)
/// - Channel master key
/// - Job ID and direction
pub fn decrypt_blob(
    encrypted: &EncryptedBlob,
    channel_keys: &ChannelKeys,
    job_id: &str,
    direction: EncryptionDirection,
) -> Result<Vec<u8>, JobCryptoError> {
    if encrypted.version != ENCRYPTED_BLOB_VERSION {
        return Err(JobCryptoError::UnsupportedVersion(encrypted.version));
    }

    // Reconstruct ephemeral public key
    let ephemeral_public = X25519PublicKey::from(encrypted.ephemeral_pubkey);

    // ECDH with our static secret
    let ephemeral_shared = channel_keys
        .local_x25519_secret()
        .diffie_hellman(&ephemeral_public);

    // Derive the same job key
    let job_key = derive_job_key(
        ephemeral_shared.as_bytes(),
        channel_keys.master_key(),
        job_id,
        direction,
    )?;

    // Decrypt
    let cipher = XChaCha20Poly1305::new_from_slice(&job_key)
        .expect("32-byte key is valid for XChaCha20-Poly1305");

    let nonce = XNonce::from_slice(&encrypted.nonce);

    cipher
        .decrypt(nonce, encrypted.ciphertext.as_ref())
        .map_err(|_| JobCryptoError::DecryptionFailed)
}

/// Derive the per-job encryption key.
///
/// Combines ephemeral ECDH output with channel master key for both
/// forward secrecy and payment binding.
fn derive_job_key(
    ephemeral_shared: &[u8],
    channel_master: &[u8; 32],
    job_id: &str,
    direction: EncryptionDirection,
) -> Result<[u8; 32], JobCryptoError> {
    // Concatenate ephemeral shared secret and channel master key
    let mut ikm = Vec::with_capacity(ephemeral_shared.len() + channel_master.len());
    ikm.extend_from_slice(ephemeral_shared);
    ikm.extend_from_slice(channel_master);

    // Use job_id as salt for unique keys per job
    let hkdf = Hkdf::<Sha256>::new(Some(job_id.as_bytes()), &ikm);

    let mut job_key = [0u8; 32];
    hkdf.expand(direction.hkdf_info(), &mut job_key)
        .map_err(|_| JobCryptoError::HkdfError)?;

    Ok(job_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_channel_keys() -> (ChannelKeys, ChannelKeys) {
        let user_secret = [1u8; 32];
        let worker_secret = [2u8; 32];

        let user_signing = ed25519_dalek::SigningKey::from_bytes(&user_secret);
        let worker_signing = ed25519_dalek::SigningKey::from_bytes(&worker_secret);

        let user_public = user_signing.verifying_key().to_bytes();
        let worker_public = worker_signing.verifying_key().to_bytes();

        let channel_pda = [3u8; 32];

        let user_keys = ChannelKeys::derive(&user_secret, &worker_public, &channel_pda).unwrap();
        let worker_keys = ChannelKeys::derive(&worker_secret, &user_public, &channel_pda).unwrap();

        (user_keys, worker_keys)
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let (user_keys, worker_keys) = test_channel_keys();
        let job_id = "test-job-123";
        let plaintext = b"Hello, confidential world!";

        // User encrypts input
        let encrypted =
            encrypt_blob(plaintext, &user_keys, job_id, EncryptionDirection::Input).unwrap();

        // Worker decrypts
        let decrypted =
            decrypt_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_blob_serialization() {
        let (user_keys, _) = test_channel_keys();

        let encrypted = encrypt_blob(
            b"test data",
            &user_keys,
            "job-1",
            EncryptionDirection::Input,
        )
        .unwrap();

        let bytes = encrypted.to_bytes();
        let deserialized = EncryptedBlob::from_bytes(&bytes).unwrap();

        assert_eq!(deserialized.version, encrypted.version);
        assert_eq!(deserialized.ephemeral_pubkey, encrypted.ephemeral_pubkey);
        assert_eq!(deserialized.nonce, encrypted.nonce);
        assert_eq!(deserialized.ciphertext, encrypted.ciphertext);
    }

    #[test]
    fn test_different_jobs_different_ciphertext() {
        let (user_keys, _) = test_channel_keys();
        let plaintext = b"same plaintext";

        let encrypted_1 =
            encrypt_blob(plaintext, &user_keys, "job-1", EncryptionDirection::Input).unwrap();
        let encrypted_2 =
            encrypt_blob(plaintext, &user_keys, "job-2", EncryptionDirection::Input).unwrap();

        // Different job IDs should produce different ciphertext
        // (even for same plaintext, due to random nonce and different key)
        assert_ne!(encrypted_1.ciphertext, encrypted_2.ciphertext);
    }

    #[test]
    fn test_bidirectional_encryption() {
        let (user_keys, worker_keys) = test_channel_keys();
        let job_id = "test-job";

        // User encrypts input for worker
        let input = b"user input data";
        let encrypted_input =
            encrypt_blob(input, &user_keys, job_id, EncryptionDirection::Input).unwrap();

        let decrypted_input = decrypt_blob(
            &encrypted_input,
            &worker_keys,
            job_id,
            EncryptionDirection::Input,
        )
        .unwrap();
        assert_eq!(decrypted_input, input);

        // Worker encrypts output for user
        let output = b"worker output data";
        let encrypted_output =
            encrypt_blob(output, &worker_keys, job_id, EncryptionDirection::Output).unwrap();

        let decrypted_output = decrypt_blob(
            &encrypted_output,
            &user_keys,
            job_id,
            EncryptionDirection::Output,
        )
        .unwrap();
        assert_eq!(decrypted_output, output);
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let (user_keys, worker_keys) = test_channel_keys();
        let job_id = "test-job";

        let mut encrypted =
            encrypt_blob(b"secret", &user_keys, job_id, EncryptionDirection::Input).unwrap();

        // Tamper with ciphertext
        if let Some(byte) = encrypted.ciphertext.get_mut(0) {
            *byte ^= 0xFF;
        }

        let result = decrypt_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input);
        assert!(matches!(result, Err(JobCryptoError::DecryptionFailed)));
    }

    #[test]
    fn test_large_blob() {
        let (user_keys, worker_keys) = test_channel_keys();
        let job_id = "large-job";

        // Test with 1MB of data
        let plaintext: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

        let encrypted =
            encrypt_blob(&plaintext, &user_keys, job_id, EncryptionDirection::Input).unwrap();

        let decrypted =
            decrypt_blob(&encrypted, &worker_keys, job_id, EncryptionDirection::Input).unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
