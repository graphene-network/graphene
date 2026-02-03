//! Channel key derivation from Ed25519 identities.
//!
//! Converts Ed25519 signing keys to X25519 keys for ECDH, then derives
//! a channel master key bound to the Solana payment channel PDA.

use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Errors during Ed25519 to X25519 conversion.
#[derive(Debug, thiserror::Error)]
pub enum Ed25519ToX25519Error {
    #[error("Invalid Ed25519 secret key length")]
    InvalidSecretKeyLength,

    #[error("Invalid Ed25519 public key length")]
    InvalidPublicKeyLength,

    #[error("Public key is a low-order point (invalid for ECDH)")]
    LowOrderPoint,
}

/// Channel keys derived from the payment channel relationship.
///
/// Contains both the shared channel master key and our X25519 static secret
/// for per-job ephemeral key exchanges.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ChannelKeys {
    /// Shared channel master key (32 bytes).
    /// Derived via ECDH + HKDF with channel PDA as salt.
    channel_master_key: [u8; 32],

    /// Our X25519 static secret for ECDH with ephemeral keys.
    #[zeroize(skip)] // X25519StaticSecret implements Drop but not Zeroize trait
    local_x25519_secret: X25519StaticSecret,

    /// Peer's X25519 public key.
    peer_x25519_public: X25519PublicKey,
}

impl std::fmt::Debug for ChannelKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelKeys")
            .field("channel_master_key", &"[REDACTED]")
            .field("local_x25519_secret", &"[REDACTED]")
            .field("peer_x25519_public", &self.peer_x25519_public.as_bytes())
            .finish()
    }
}

impl ChannelKeys {
    /// Derive channel keys from Ed25519 identities and channel PDA.
    ///
    /// # Algorithm
    ///
    /// 1. Convert Ed25519 secret → X25519 secret (via SHA512 clamping)
    /// 2. Convert Ed25519 public → X25519 public (via curve conversion)
    /// 3. Perform X25519 ECDH to get shared secret
    /// 4. HKDF-SHA256(shared_secret, salt=channel_pda, info="graphene-channel-v1")
    ///
    /// # Arguments
    ///
    /// * `local_ed25519_secret` - Our Ed25519 signing key (32 bytes)
    /// * `peer_ed25519_public` - Peer's Ed25519 verifying key (32 bytes)
    /// * `channel_pda` - Solana PDA for the payment channel (binds key to channel)
    pub fn derive(
        local_ed25519_secret: &[u8; 32],
        peer_ed25519_public: &[u8; 32],
        channel_pda: &[u8; 32],
    ) -> Result<Self, Ed25519ToX25519Error> {
        // Convert Ed25519 secret to X25519 secret
        let local_x25519_secret = ed25519_secret_to_x25519(local_ed25519_secret)?;

        // Convert Ed25519 public to X25519 public
        let peer_x25519_public = ed25519_public_to_x25519(peer_ed25519_public)?;

        // Perform ECDH
        let shared_secret = local_x25519_secret.diffie_hellman(&peer_x25519_public);

        // Derive channel master key via HKDF
        let hkdf = Hkdf::<Sha256>::new(Some(channel_pda), shared_secret.as_bytes());
        let mut channel_master_key = [0u8; 32];
        hkdf.expand(b"graphene-channel-v1", &mut channel_master_key)
            .expect("32 bytes is valid HKDF output length");

        Ok(Self {
            channel_master_key,
            local_x25519_secret,
            peer_x25519_public,
        })
    }

    /// Get the channel master key for key derivation.
    pub fn master_key(&self) -> &[u8; 32] {
        &self.channel_master_key
    }

    /// Get our X25519 static secret for ephemeral ECDH.
    pub fn local_x25519_secret(&self) -> &X25519StaticSecret {
        &self.local_x25519_secret
    }

    /// Get peer's X25519 public key.
    pub fn peer_x25519_public(&self) -> &X25519PublicKey {
        &self.peer_x25519_public
    }
}

/// Convert Ed25519 secret key to X25519 secret key.
///
/// Uses the standard algorithm from RFC 8032 / libsodium:
/// 1. SHA512(secret_key)
/// 2. Take first 32 bytes
/// 3. Clamp (done automatically by x25519-dalek)
fn ed25519_secret_to_x25519(
    ed25519_secret: &[u8; 32],
) -> Result<X25519StaticSecret, Ed25519ToX25519Error> {
    use sha2::{Digest, Sha512};

    // Hash the Ed25519 secret key with SHA512
    let hash = Sha512::digest(ed25519_secret);

    // Take the first 32 bytes as the X25519 secret
    let mut x25519_bytes = [0u8; 32];
    x25519_bytes.copy_from_slice(&hash[..32]);

    // x25519-dalek automatically clamps the scalar
    Ok(X25519StaticSecret::from(x25519_bytes))
}

/// Convert Ed25519 public key to X25519 public key.
///
/// Uses the birational map from the Ed25519 curve to Curve25519.
/// This is the same algorithm as libsodium's `crypto_sign_ed25519_pk_to_curve25519`.
fn ed25519_public_to_x25519(
    ed25519_public: &[u8; 32],
) -> Result<X25519PublicKey, Ed25519ToX25519Error> {
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use curve25519_dalek::montgomery::MontgomeryPoint;

    // Decompress the Ed25519 point
    let compressed = CompressedEdwardsY::from_slice(ed25519_public)
        .map_err(|_| Ed25519ToX25519Error::InvalidPublicKeyLength)?;

    let edwards_point = compressed
        .decompress()
        .ok_or(Ed25519ToX25519Error::LowOrderPoint)?;

    // Check for low-order points (security requirement)
    if edwards_point.is_small_order() {
        return Err(Ed25519ToX25519Error::LowOrderPoint);
    }

    // Convert to Montgomery form (Curve25519)
    let montgomery: MontgomeryPoint = edwards_point.to_montgomery();

    Ok(X25519PublicKey::from(montgomery.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_conversion_roundtrip() {
        // Generate a test Ed25519 keypair
        let secret_bytes = [42u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret_bytes);
        let public_bytes = signing_key.verifying_key().to_bytes();

        // Convert to X25519
        let x25519_secret = ed25519_secret_to_x25519(&secret_bytes).unwrap();
        let x25519_public = ed25519_public_to_x25519(&public_bytes).unwrap();

        // Verify the public key matches what we'd derive from the secret
        let derived_public = X25519PublicKey::from(&x25519_secret);
        assert_eq!(x25519_public.as_bytes(), derived_public.as_bytes());
    }

    #[test]
    fn test_channel_key_symmetry() {
        // Two parties with different keys should derive the same channel master key
        let alice_secret = [1u8; 32];
        let bob_secret = [2u8; 32];

        let alice_signing = ed25519_dalek::SigningKey::from_bytes(&alice_secret);
        let bob_signing = ed25519_dalek::SigningKey::from_bytes(&bob_secret);

        let alice_public = alice_signing.verifying_key().to_bytes();
        let bob_public = bob_signing.verifying_key().to_bytes();

        let channel_pda = [3u8; 32];

        // Alice derives keys with Bob
        let alice_keys = ChannelKeys::derive(&alice_secret, &bob_public, &channel_pda).unwrap();

        // Bob derives keys with Alice
        let bob_keys = ChannelKeys::derive(&bob_secret, &alice_public, &channel_pda).unwrap();

        // They should have the same channel master key
        assert_eq!(alice_keys.master_key(), bob_keys.master_key());
    }

    #[test]
    fn test_different_channels_different_keys() {
        let alice_secret = [1u8; 32];
        let bob_secret = [2u8; 32];

        let alice_signing = ed25519_dalek::SigningKey::from_bytes(&alice_secret);
        let bob_public = alice_signing.verifying_key().to_bytes();
        let bob_signing = ed25519_dalek::SigningKey::from_bytes(&bob_secret);
        let _ = bob_signing.verifying_key().to_bytes();

        let channel_pda_1 = [3u8; 32];
        let channel_pda_2 = [4u8; 32];

        let keys_1 = ChannelKeys::derive(&alice_secret, &bob_public, &channel_pda_1).unwrap();
        let keys_2 = ChannelKeys::derive(&alice_secret, &bob_public, &channel_pda_2).unwrap();

        // Different channels should produce different keys
        assert_ne!(keys_1.master_key(), keys_2.master_key());
    }
}
