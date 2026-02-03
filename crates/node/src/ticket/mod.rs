//! Off-chain payment ticket format for zero-latency job payments.
//!
//! This module implements the payment ticket specification from Issue #27,
//! providing bincode-serialized tickets with Ed25519 signatures for worker-side
//! validation.
//!
//! # Design
//!
//! The ticket format separates concerns:
//! - **Signed payload** (48 bytes): `channel_id || amount_micros || nonce`
//!   - This format is compatible with on-chain Ed25519 verification
//!   - Same signature works for both off-chain validation and on-chain settlement
//! - **Unsigned envelope**: Contains timestamp for operational staleness checks
//!
//! # Validation Rules
//!
//! Workers validate tickets according to these rules (in order):
//! 1. **Signature** - Ed25519 signature must be valid for payer's pubkey
//! 2. **Nonce** - Must be strictly greater than last seen nonce (replay protection)
//! 3. **Amount** - Must be >= last amount (cumulative) and <= channel balance
//! 4. **Timestamp** - Must be within ±5 minutes of current time (staleness)
//!
//! # Example
//!
//! ```text
//! use monad_node::ticket::{
//!     DefaultTicketSigner, DefaultTicketValidator, TicketSigner, TicketValidator,
//!     ChannelState,
//! };
//!
//! // User signs a ticket
//! let signer = DefaultTicketSigner::from_bytes(&user_secret_key);
//! let ticket = signer.sign_ticket(channel_id, amount, nonce).await?;
//!
//! // Worker validates the ticket
//! let validator = DefaultTicketValidator::new();
//! let channel_state = ChannelState {
//!     last_nonce: 0,
//!     last_amount: 0,
//!     channel_balance: 10_000_000,
//! };
//! validator.validate(&ticket, &user_pubkey, &channel_state).await?;
//! ```

mod mock;
mod signer;
mod types;
mod validator;

pub use mock::{MockTicketValidator, MockValidatorBehavior};
pub use signer::{DefaultTicketSigner, TicketSigner};
pub use types::{ChannelState, PaymentTicket, Signature64, TicketPayload};
pub use validator::{
    DefaultTicketValidator, TicketValidator, MAX_FUTURE_TIMESTAMP_SECS, MAX_STALE_TIMESTAMP_SECS,
};

/// Errors that can occur during ticket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TicketError {
    /// Ed25519 signature verification failed.
    #[error("invalid signature")]
    InvalidSignature,

    /// Public key bytes are not a valid Ed25519 point.
    #[error("invalid public key")]
    InvalidPublicKey,

    /// Ticket nonce has already been seen (replay attack).
    #[error("replayed nonce: ticket={ticket_nonce}, last={last_nonce}")]
    ReplayedNonce { ticket_nonce: u64, last_nonce: u64 },

    /// Ticket amount is less than previously seen (not cumulative).
    #[error("non-cumulative amount: ticket={ticket_amount}, last={last_amount}")]
    NonCumulativeAmount {
        ticket_amount: u64,
        last_amount: u64,
    },

    /// Ticket amount exceeds channel balance.
    #[error("insufficient balance: ticket={ticket_amount}, balance={channel_balance}")]
    InsufficientBalance {
        ticket_amount: u64,
        channel_balance: u64,
    },

    /// Ticket timestamp is too far in the future.
    #[error("future timestamp: ticket={ticket_time}, current={current_time}")]
    FutureTimestamp { ticket_time: i64, current_time: i64 },

    /// Ticket timestamp is too old (stale).
    #[error("stale timestamp: ticket={ticket_time}, current={current_time}")]
    StaleTimestamp { ticket_time: i64, current_time: i64 },

    /// System time error.
    #[error("system time error")]
    TimestampError,

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    SerializationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test: full roundtrip from signing to validation.
    #[tokio::test]
    async fn test_sign_and_validate_roundtrip() {
        // User creates and signs a ticket
        let user_secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&user_secret);

        let channel_id = [1u8; 32];
        let amount_micros = 1_000_000;
        let nonce = 5;

        let ticket = signer
            .sign_ticket(channel_id, amount_micros, nonce)
            .await
            .expect("signing failed");

        // Worker validates the ticket
        let validator = DefaultTicketValidator::new();
        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(result.is_ok(), "valid ticket should pass validation");
    }

    /// Test that the 48-byte payload format is correct for on-chain compatibility.
    #[test]
    fn test_payload_format_on_chain_compatible() {
        let payload = TicketPayload {
            channel_id: [0x11u8; 32],
            amount_micros: 0x0102030405060708,
            nonce: 0x0A0B0C0D0E0F1011,
        };

        let bytes = payload.to_bytes();

        // Verify size
        assert_eq!(bytes.len(), 48);

        // Verify channel_id is first 32 bytes
        assert_eq!(&bytes[0..32], &[0x11u8; 32]);

        // Verify amount_micros is little-endian bytes 32-40
        assert_eq!(&bytes[32..40], &0x0102030405060708u64.to_le_bytes());

        // Verify nonce is little-endian bytes 40-48
        assert_eq!(&bytes[40..48], &0x0A0B0C0D0E0F1011u64.to_le_bytes());
    }
}
