//! Payment ticket types for off-chain job payments.
//!
//! The ticket format separates signed payload from unsigned envelope metadata:
//! - **Signed payload** (48 bytes): `channel_id || amount_micros || nonce`
//!   - Compatible with on-chain Ed25519 verification
//! - **Unsigned envelope**: Includes timestamp for operational staleness checks

use serde::{Deserialize, Serialize};

/// A 64-byte signature with proper serde support.
///
/// Serde doesn't implement Serialize/Deserialize for `[u8; 64]` by default,
/// so we use a wrapper type with custom serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature64(pub [u8; 64]);

impl Signature64 {
    /// Get the signature bytes.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl From<[u8; 64]> for Signature64 {
    fn from(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl From<Signature64> for [u8; 64] {
    fn from(sig: Signature64) -> Self {
        sig.0
    }
}

impl Serialize for Signature64 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Use slice serialization for bincode compatibility (not serialize_bytes)
        self.0.as_slice().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Signature64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        let arr: [u8; 64] = vec.try_into().map_err(|v: Vec<u8>| {
            serde::de::Error::custom(format!("expected 64 bytes, got {}", v.len()))
        })?;
        Ok(Self(arr))
    }
}

/// 48-byte payload that gets signed for on-chain compatible verification.
///
/// This is the exact format verified by `programs/graphene/src/utils/ed25519.rs`:
/// - 32 bytes: channel_id
/// - 8 bytes: amount_micros (little-endian u64)
/// - 8 bytes: nonce (little-endian u64)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketPayload {
    /// Payment channel address (Solana pubkey).
    pub channel_id: [u8; 32],

    /// Cumulative amount authorized in microtokens.
    pub amount_micros: u64,

    /// Monotonically increasing nonce (prevents replay).
    pub nonce: u64,
}

impl TicketPayload {
    /// Size of the serialized payload in bytes.
    pub const SIZE: usize = 48;

    /// Serialize to 48-byte format for signing/verification.
    ///
    /// Format: `channel_id (32) || amount_micros (8) || nonce (8)`
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..32].copy_from_slice(&self.channel_id);
        bytes[32..40].copy_from_slice(&self.amount_micros.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.nonce.to_le_bytes());
        bytes
    }

    /// Deserialize from 48-byte format.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut channel_id = [0u8; 32];
        channel_id.copy_from_slice(&bytes[0..32]);

        let amount_micros = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let nonce = u64::from_le_bytes(bytes[40..48].try_into().unwrap());

        Self {
            channel_id,
            amount_micros,
            nonce,
        }
    }
}

/// Full payment ticket with envelope metadata.
///
/// The signature covers only the 48-byte payload (channel_id, amount_micros, nonce).
/// Timestamp is unsigned envelope metadata for operational staleness checks only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTicket {
    /// Payment channel address (Solana pubkey).
    pub channel_id: [u8; 32],

    /// Cumulative amount authorized in microtokens.
    pub amount_micros: u64,

    /// Monotonically increasing nonce (prevents replay).
    pub nonce: u64,

    /// Timestamp of ticket creation (Unix epoch seconds).
    /// **Unsigned** - only used for staleness checks, not part of signature.
    pub timestamp: i64,

    /// Ed25519 signature over the 48-byte payload.
    signature: Signature64,
}

impl PaymentTicket {
    /// Create a new payment ticket.
    pub fn new(
        channel_id: [u8; 32],
        amount_micros: u64,
        nonce: u64,
        timestamp: i64,
        signature: [u8; 64],
    ) -> Self {
        Self {
            channel_id,
            amount_micros,
            nonce,
            timestamp,
            signature: Signature64(signature),
        }
    }

    /// Get the signature bytes.
    pub fn signature(&self) -> &[u8; 64] {
        self.signature.as_bytes()
    }
}

impl PaymentTicket {
    /// Extract the signed payload from this ticket.
    pub fn payload(&self) -> TicketPayload {
        TicketPayload {
            channel_id: self.channel_id,
            amount_micros: self.amount_micros,
            nonce: self.nonce,
        }
    }

    /// Get the 48-byte message that was signed.
    pub fn signed_message(&self) -> [u8; TicketPayload::SIZE] {
        self.payload().to_bytes()
    }
}

/// Channel state for validation context.
///
/// Workers track this state per-channel to validate incoming tickets.
#[derive(Debug, Clone, Default)]
pub struct ChannelState {
    /// Last seen nonce for this channel.
    pub last_nonce: u64,

    /// Last cumulative amount seen.
    pub last_amount: u64,

    /// Total balance available in the channel.
    pub channel_balance: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_roundtrip() {
        let payload = TicketPayload {
            channel_id: [42u8; 32],
            amount_micros: 1_000_000,
            nonce: 5,
        };

        let bytes = payload.to_bytes();
        let recovered = TicketPayload::from_bytes(&bytes);

        assert_eq!(payload.channel_id, recovered.channel_id);
        assert_eq!(payload.amount_micros, recovered.amount_micros);
        assert_eq!(payload.nonce, recovered.nonce);
    }

    #[test]
    fn test_payload_size() {
        assert_eq!(TicketPayload::SIZE, 48);

        let payload = TicketPayload {
            channel_id: [0u8; 32],
            amount_micros: u64::MAX,
            nonce: u64::MAX,
        };
        assert_eq!(payload.to_bytes().len(), 48);
    }

    #[test]
    fn test_ticket_payload_extraction() {
        let ticket = PaymentTicket::new([1u8; 32], 500_000, 10, 1700000000, [0u8; 64]);

        let payload = ticket.payload();
        assert_eq!(payload.channel_id, ticket.channel_id);
        assert_eq!(payload.amount_micros, ticket.amount_micros);
        assert_eq!(payload.nonce, ticket.nonce);
    }

    #[test]
    fn test_ticket_serde_roundtrip() {
        let ticket = PaymentTicket::new([42u8; 32], 1_000_000, 5, 1700000000, [0xAB; 64]);

        let json = serde_json::to_string(&ticket).expect("serialize failed");
        let recovered: PaymentTicket = serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(ticket.channel_id, recovered.channel_id);
        assert_eq!(ticket.amount_micros, recovered.amount_micros);
        assert_eq!(ticket.nonce, recovered.nonce);
        assert_eq!(ticket.timestamp, recovered.timestamp);
        assert_eq!(ticket.signature(), recovered.signature());
    }
}
