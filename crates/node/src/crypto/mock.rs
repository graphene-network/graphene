//! Mock crypto provider for testing.
//!
//! Provides a configurable mock that can simulate various failure modes.

use super::{
    channel_keys::ChannelKeys, decrypt_blob, encrypt_blob, CryptoError, CryptoProvider,
    EncryptedBlob, EncryptionDirection,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Configurable behavior for mock crypto operations.
#[derive(Debug, Clone, Default)]
pub enum MockCryptoBehavior {
    /// Normal operation - encryption/decryption works correctly
    #[default]
    Normal,

    /// Fail all operations with a specific error
    AlwaysFail(String),

    /// Fail after N successful operations
    FailAfter(usize),

    /// Return corrupted ciphertext (decryption will fail)
    CorruptCiphertext,
}

/// Mock implementation of CryptoProvider for testing.
#[derive(Debug, Clone)]
pub struct MockCryptoProvider {
    behavior: MockCryptoBehavior,
    operation_count: Arc<AtomicUsize>,
}

impl Default for MockCryptoProvider {
    fn default() -> Self {
        Self::new(MockCryptoBehavior::Normal)
    }
}

impl MockCryptoProvider {
    /// Create a new mock with specified behavior.
    pub fn new(behavior: MockCryptoBehavior) -> Self {
        Self {
            behavior,
            operation_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a mock that works normally.
    pub fn working() -> Self {
        Self::new(MockCryptoBehavior::Normal)
    }

    /// Create a mock that always fails.
    pub fn failing(error: impl Into<String>) -> Self {
        Self::new(MockCryptoBehavior::AlwaysFail(error.into()))
    }

    /// Get the number of operations performed.
    pub fn operation_count(&self) -> usize {
        self.operation_count.load(Ordering::SeqCst)
    }

    fn check_should_fail(&self) -> Result<(), CryptoError> {
        let count = self.operation_count.fetch_add(1, Ordering::SeqCst);

        match &self.behavior {
            MockCryptoBehavior::Normal => Ok(()),
            MockCryptoBehavior::AlwaysFail(msg) => Err(CryptoError::JobCrypto(
                super::JobCryptoError::InvalidFormat(msg.clone()),
            )),
            MockCryptoBehavior::FailAfter(n) if count >= *n => Err(CryptoError::JobCrypto(
                super::JobCryptoError::InvalidFormat(format!("Failed after {} operations", n)),
            )),
            MockCryptoBehavior::FailAfter(_) => Ok(()),
            MockCryptoBehavior::CorruptCiphertext => Ok(()), // Handled in encrypt
        }
    }
}

#[async_trait]
impl CryptoProvider for MockCryptoProvider {
    fn derive_channel_keys(
        &self,
        local_ed25519_secret: &[u8; 32],
        peer_ed25519_public: &[u8; 32],
        channel_pda: &[u8; 32],
    ) -> Result<ChannelKeys, CryptoError> {
        self.check_should_fail()?;
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
        self.check_should_fail()?;

        let mut encrypted = encrypt_blob(plaintext, channel_keys, job_id, direction)?;

        // Optionally corrupt the ciphertext
        if matches!(self.behavior, MockCryptoBehavior::CorruptCiphertext) {
            if let Some(byte) = encrypted.ciphertext.get_mut(0) {
                *byte ^= 0xFF;
            }
        }

        Ok(encrypted)
    }

    fn decrypt_job_blob(
        &self,
        encrypted: &EncryptedBlob,
        channel_keys: &ChannelKeys,
        job_id: &str,
        direction: EncryptionDirection,
    ) -> Result<Vec<u8>, CryptoError> {
        self.check_should_fail()?;
        Ok(decrypt_blob(encrypted, channel_keys, job_id, direction)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keys() -> (ChannelKeys, ChannelKeys) {
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
    fn test_mock_normal_behavior() {
        let mock = MockCryptoProvider::working();
        let (user_keys, worker_keys) = test_keys();

        let encrypted = mock
            .encrypt_job_blob(
                b"test",
                &user_keys,
                "job-1",
                EncryptionDirection::Input,
            )
            .unwrap();

        let decrypted = mock
            .decrypt_job_blob(&encrypted, &worker_keys, "job-1", EncryptionDirection::Input)
            .unwrap();

        assert_eq!(decrypted, b"test");
        assert_eq!(mock.operation_count(), 2);
    }

    #[test]
    fn test_mock_always_fail() {
        let mock = MockCryptoProvider::failing("test error");
        let (user_keys, _) = test_keys();

        let result = mock.encrypt_job_blob(
            b"test",
            &user_keys,
            "job-1",
            EncryptionDirection::Input,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_mock_fail_after() {
        let mock = MockCryptoProvider::new(MockCryptoBehavior::FailAfter(2));
        let (user_keys, _) = test_keys();

        // First two operations succeed
        assert!(mock
            .encrypt_job_blob(b"test", &user_keys, "job-1", EncryptionDirection::Input)
            .is_ok());
        assert!(mock
            .encrypt_job_blob(b"test", &user_keys, "job-2", EncryptionDirection::Input)
            .is_ok());

        // Third operation fails
        assert!(mock
            .encrypt_job_blob(b"test", &user_keys, "job-3", EncryptionDirection::Input)
            .is_err());
    }

    #[test]
    fn test_mock_corrupt_ciphertext() {
        let mock = MockCryptoProvider::new(MockCryptoBehavior::CorruptCiphertext);
        let (user_keys, worker_keys) = test_keys();

        let encrypted = mock
            .encrypt_job_blob(
                b"test",
                &user_keys,
                "job-1",
                EncryptionDirection::Input,
            )
            .unwrap();

        // Decryption should fail due to corrupted ciphertext
        let result =
            mock.decrypt_job_blob(&encrypted, &worker_keys, "job-1", EncryptionDirection::Input);

        assert!(result.is_err());
    }
}
