//! Mock ticket validator for testing.
//!
//! Provides configurable behavior for unit tests without cryptographic operations.

use super::{ChannelState, PaymentTicket, TicketError, TicketValidator};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Configurable mock behavior for ticket validation.
#[derive(Debug, Clone, Default)]
pub enum MockValidatorBehavior {
    /// Always accept tickets.
    #[default]
    AlwaysValid,

    /// Always reject with invalid signature.
    AlwaysInvalidSignature,

    /// Always reject with replayed nonce.
    AlwaysReplayedNonce,

    /// Always reject with insufficient balance.
    AlwaysInsufficientBalance,

    /// Always reject with stale timestamp.
    AlwaysStaleTimestamp,

    /// Accept the first N tickets, then reject.
    AcceptFirst(usize),

    /// Custom error to return.
    CustomError(TicketError),
}

/// Mock ticket validator for testing.
///
/// Tracks validation calls and returns configurable results.
#[derive(Debug, Clone)]
pub struct MockTicketValidator {
    behavior: MockValidatorBehavior,
    call_count: Arc<AtomicUsize>,
}

impl Default for MockTicketValidator {
    fn default() -> Self {
        Self::new(MockValidatorBehavior::AlwaysValid)
    }
}

impl MockTicketValidator {
    /// Create a new mock validator with the specified behavior.
    pub fn new(behavior: MockValidatorBehavior) -> Self {
        Self {
            behavior,
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a mock that always accepts tickets.
    pub fn always_valid() -> Self {
        Self::new(MockValidatorBehavior::AlwaysValid)
    }

    /// Create a mock that always rejects with invalid signature.
    pub fn always_invalid_signature() -> Self {
        Self::new(MockValidatorBehavior::AlwaysInvalidSignature)
    }

    /// Create a mock that accepts the first N tickets.
    pub fn accept_first(n: usize) -> Self {
        Self::new(MockValidatorBehavior::AcceptFirst(n))
    }

    /// Get the number of times `validate` was called.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Reset the call counter.
    pub fn reset_call_count(&self) {
        self.call_count.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl TicketValidator for MockTicketValidator {
    async fn validate(
        &self,
        _ticket: &PaymentTicket,
        _payer_pubkey: &[u8; 32],
        _channel_state: &ChannelState,
    ) -> Result<(), TicketError> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);

        match &self.behavior {
            MockValidatorBehavior::AlwaysValid => Ok(()),

            MockValidatorBehavior::AlwaysInvalidSignature => Err(TicketError::InvalidSignature),

            MockValidatorBehavior::AlwaysReplayedNonce => Err(TicketError::ReplayedNonce {
                ticket_nonce: 1,
                last_nonce: 2,
            }),

            MockValidatorBehavior::AlwaysInsufficientBalance => {
                Err(TicketError::InsufficientBalance {
                    ticket_amount: 1000,
                    channel_balance: 500,
                })
            }

            MockValidatorBehavior::AlwaysStaleTimestamp => Err(TicketError::StaleTimestamp {
                ticket_time: 0,
                current_time: 1000,
            }),

            MockValidatorBehavior::AcceptFirst(n) => {
                if count < *n {
                    Ok(())
                } else {
                    Err(TicketError::InvalidSignature)
                }
            }

            MockValidatorBehavior::CustomError(err) => Err(err.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_ticket() -> PaymentTicket {
        PaymentTicket::new([0u8; 32], 1000, 1, 0, [0u8; 64])
    }

    fn dummy_channel_state() -> ChannelState {
        ChannelState::default()
    }

    #[tokio::test]
    async fn test_always_valid() {
        let validator = MockTicketValidator::always_valid();
        let result = validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_always_invalid_signature() {
        let validator = MockTicketValidator::always_invalid_signature();
        let result = validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await;
        assert!(matches!(result, Err(TicketError::InvalidSignature)));
    }

    #[tokio::test]
    async fn test_accept_first_n() {
        let validator = MockTicketValidator::accept_first(2);

        // First two should pass
        assert!(validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await
            .is_ok());
        assert!(validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await
            .is_ok());

        // Third should fail
        assert!(validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_call_count() {
        let validator = MockTicketValidator::always_valid();

        assert_eq!(validator.call_count(), 0);

        validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await
            .ok();
        assert_eq!(validator.call_count(), 1);

        validator
            .validate(&dummy_ticket(), &[0u8; 32], &dummy_channel_state())
            .await
            .ok();
        assert_eq!(validator.call_count(), 2);

        validator.reset_call_count();
        assert_eq!(validator.call_count(), 0);
    }
}
