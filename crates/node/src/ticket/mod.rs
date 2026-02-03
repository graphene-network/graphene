//! Off-chain payment ticket format for zero-latency job payments.
//!
//! This module implements:
//! - **Payment tickets** (Issue #27): Bincode-serialized tickets with Ed25519 signatures
//! - **Channel state management** (Issue #28): Worker-side local state tracking for <1ms validation
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TICKET VERIFICATION                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  1. Signature Check (<0.1ms) - Ed25519 verify               │
//! │  2. Local State Check (<0.1ms) - nonce, amount, balance     │
//! │  3. Timestamp Check (<0.1ms) - staleness window             │
//! │  Total: <1ms verification                                    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
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
//! # Channel State Management
//!
//! Workers maintain local state for each payment channel to enable zero-latency
//! ticket verification without on-chain lookups:
//!
//! - [`ChannelLocalState`] - Local tracking of channel balance, nonce, and settlement
//! - [`ChannelStateManager`] - Trait for state management implementations
//! - [`DefaultChannelStateManager`] - In-memory implementation with `Arc<RwLock<HashMap>>`
//! - [`ChannelSyncService`] - Background service for periodic sync and threshold monitoring
//!
//! # Background Services
//!
//! The [`ChannelSyncService`] provides:
//! - Periodic on-chain sync (configurable interval, default 10 min)
//! - Threshold monitoring for auto-settlement triggers
//! - WebSocket subscriptions for real-time channel updates
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

mod channel_manager;
mod channel_state;
mod channel_sync;
mod mock;
mod signer;
mod solana_client;
mod types;
mod validator;

pub use channel_manager::{
    DefaultChannelStateManager, MockChannelBehavior, MockChannelStateManager,
};
pub use channel_state::{
    ChannelConfig, ChannelError, ChannelEvent, ChannelLocalState, ChannelStateManager,
    OnChainChannelState, TicketAcceptResult,
};
pub use channel_sync::ChannelSyncService;
pub use mock::{MockTicketValidator, MockValidatorBehavior};
pub use signer::{DefaultTicketSigner, TicketSigner};
pub use solana_client::{
    DefaultSolanaChannelClient, MockSolanaBehavior, MockSolanaChannelClient, OnChainChannel,
    SolanaChannelClient, SolanaClientError,
};
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

    /// Benchmark test: Verify ticket validation completes in under 1ms.
    ///
    /// This test validates the performance requirement that ticket verification
    /// must complete in under 1ms to support high-throughput job processing.
    ///
    /// The test measures:
    /// - Ed25519 signature verification
    /// - Nonce validation
    /// - Amount validation
    /// - Timestamp validation
    #[tokio::test]
    #[ignore] // CI runners are too slow for sub-1ms timing assertions
    async fn bench_ticket_validation_under_1ms() {
        use std::time::Instant;

        // Setup: Create signer, validator, channel state
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);
        let validator = DefaultTicketValidator::new();
        let pubkey = signer.public_key();

        let channel_id = [1u8; 32];

        // Pre-sign tickets (to exclude signing time from benchmark)
        const ITERATIONS: usize = 1000;
        let mut tickets = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            let ticket = signer
                .sign_ticket(channel_id, 1_000_000 * (i as u64 + 1), i as u64 + 1)
                .await
                .expect("signing failed");
            tickets.push(ticket);
        }

        // Warm-up run
        let warmup_state = ChannelState {
            last_nonce: 0,
            last_amount: 0,
            channel_balance: 100_000_000_000,
        };
        validator
            .validate(&tickets[0], &pubkey, &warmup_state)
            .await
            .ok();

        // Benchmark: Run 1000 validations and measure
        let start = Instant::now();

        for (i, ticket) in tickets.iter().enumerate() {
            let state = ChannelState {
                last_nonce: i as u64,
                last_amount: 1_000_000 * i as u64,
                channel_balance: 100_000_000_000,
            };
            validator
                .validate(ticket, &pubkey, &state)
                .await
                .expect("validation failed");
        }

        let elapsed = start.elapsed();
        let avg_micros = elapsed.as_micros() as f64 / ITERATIONS as f64;

        println!(
            "Average validation time: {:.2}µs ({:.4}ms)",
            avg_micros,
            avg_micros / 1000.0
        );

        // Assert < 1ms (1000µs) per validation
        assert!(
            avg_micros < 1000.0,
            "Validation took {:.2}µs, expected <1000µs",
            avg_micros
        );
    }

    /// Benchmark test: Verify accept_ticket flow completes in under 1ms.
    ///
    /// This tests the full ticket acceptance path including:
    /// - Channel state lookup
    /// - Ticket validation
    /// - State update
    #[tokio::test]
    async fn bench_accept_ticket_under_1ms() {
        use std::sync::Arc;
        use std::time::Instant;

        // Setup: Create manager with mock validator that always succeeds
        let config = ChannelConfig::default();
        let validator = Arc::new(MockTicketValidator::new(MockValidatorBehavior::AlwaysValid));
        let manager = DefaultChannelStateManager::new(config, validator);

        let channel_id = [1u8; 32];

        // Insert a channel with large balance
        let state = ChannelLocalState {
            channel_id,
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 100_000_000_000, // Large balance
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        manager.upsert_channel(state).await.unwrap();

        // Pre-create tickets (to exclude creation time from benchmark)
        const ITERATIONS: usize = 1000;
        let mut tickets = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            let ticket = PaymentTicket::new(
                channel_id,
                1_000_000 * (i as u64 + 1),
                i as u64 + 1,
                0,
                [0u8; 64],
            );
            tickets.push(ticket);
        }

        // Reset channel state for actual benchmark
        let state = ChannelLocalState {
            channel_id,
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 100_000_000_000,
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        manager.upsert_channel(state).await.unwrap();

        // Benchmark accept_ticket
        let start = Instant::now();

        for ticket in &tickets {
            let result = manager.accept_ticket(&channel_id, ticket).await.unwrap();
            assert!(
                matches!(result, TicketAcceptResult::Accepted { .. }),
                "Expected ticket to be accepted"
            );
        }

        let elapsed = start.elapsed();
        let avg_micros = elapsed.as_micros() as f64 / ITERATIONS as f64;

        println!(
            "Average accept_ticket time: {:.2}µs ({:.4}ms)",
            avg_micros,
            avg_micros / 1000.0
        );

        // Assert < 1ms (1000µs) per accept_ticket
        assert!(
            avg_micros < 1000.0,
            "accept_ticket took {:.2}µs, expected <1000µs",
            avg_micros
        );
    }

    /// Benchmark test: Verify full accept_ticket with real validator completes in under 1ms.
    ///
    /// This is the most realistic benchmark, using the actual Ed25519 validator.
    #[tokio::test]
    #[ignore] // CI runners are too slow for sub-1ms timing assertions
    async fn bench_accept_ticket_with_real_validator_under_1ms() {
        use std::time::Instant;

        // Setup: Create manager with real validator
        let config = ChannelConfig::default();
        let manager = DefaultChannelStateManager::with_default_validator(config);

        // Create signer and get public key
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);
        let user_pubkey = signer.public_key();

        let channel_id = [1u8; 32];

        // Insert a channel with matching user pubkey and large balance
        let state = ChannelLocalState {
            channel_id,
            user: user_pubkey,
            worker: [3u8; 32],
            on_chain_balance: 100_000_000_000, // Large balance
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        manager.upsert_channel(state).await.unwrap();

        // Pre-sign tickets
        const ITERATIONS: usize = 1000;
        let mut tickets = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            let ticket = signer
                .sign_ticket(channel_id, 1_000_000 * (i as u64 + 1), i as u64 + 1)
                .await
                .expect("signing failed");
            tickets.push(ticket);
        }

        // Reset channel state for benchmark
        let state = ChannelLocalState {
            channel_id,
            user: user_pubkey,
            worker: [3u8; 32],
            on_chain_balance: 100_000_000_000,
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 0,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        };
        manager.upsert_channel(state).await.unwrap();

        // Benchmark accept_ticket with real validation
        let start = Instant::now();

        for ticket in &tickets {
            let result = manager.accept_ticket(&channel_id, ticket).await.unwrap();
            assert!(
                matches!(result, TicketAcceptResult::Accepted { .. }),
                "Expected ticket to be accepted"
            );
        }

        let elapsed = start.elapsed();
        let avg_micros = elapsed.as_micros() as f64 / ITERATIONS as f64;

        println!(
            "Average accept_ticket (real validator) time: {:.2}µs ({:.4}ms)",
            avg_micros,
            avg_micros / 1000.0
        );

        // Assert < 1ms (1000µs) per accept_ticket
        assert!(
            avg_micros < 1000.0,
            "accept_ticket (real validator) took {:.2}µs, expected <1000µs",
            avg_micros
        );
    }
}
