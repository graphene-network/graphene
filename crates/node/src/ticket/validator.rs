//! Ticket validation for workers.
//!
//! Workers validate incoming payment tickets before accepting jobs.

use super::{ChannelState, PaymentTicket, TicketError};
use async_trait::async_trait;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum allowed future timestamp offset (60 seconds).
pub const MAX_FUTURE_TIMESTAMP_SECS: i64 = 60;

/// Maximum allowed past timestamp offset (300 seconds / 5 minutes).
pub const MAX_STALE_TIMESTAMP_SECS: i64 = 300;

/// Trait for validating payment tickets.
///
/// Implementations handle signature verification and business rule validation.
#[async_trait]
pub trait TicketValidator: Send + Sync {
    /// Validate a payment ticket.
    ///
    /// # Arguments
    /// * `ticket` - The ticket to validate
    /// * `payer_pubkey` - Expected Ed25519 public key of the payer
    /// * `channel_state` - Current state of the payment channel
    ///
    /// # Returns
    /// `Ok(())` if the ticket is valid, or a specific error describing the failure.
    async fn validate(
        &self,
        ticket: &PaymentTicket,
        payer_pubkey: &[u8; 32],
        channel_state: &ChannelState,
    ) -> Result<(), TicketError>;
}

/// Default ticket validator using Ed25519 signature verification.
#[derive(Debug, Default, Clone)]
pub struct DefaultTicketValidator;

impl DefaultTicketValidator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self
    }

    /// Verify the Ed25519 signature over the 48-byte payload.
    fn verify_signature(
        &self,
        ticket: &PaymentTicket,
        payer_pubkey: &[u8; 32],
    ) -> Result<(), TicketError> {
        let verifying_key =
            VerifyingKey::from_bytes(payer_pubkey).map_err(|_| TicketError::InvalidPublicKey)?;

        let signature = Signature::from_bytes(ticket.signature());

        let message = ticket.signed_message();

        verifying_key
            .verify(&message, &signature)
            .map_err(|_| TicketError::InvalidSignature)
    }

    /// Validate timestamp is within acceptable bounds.
    fn validate_timestamp(&self, ticket: &PaymentTicket) -> Result<(), TicketError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TicketError::TimestampError)?
            .as_secs() as i64;

        // Reject future timestamps (clock skew protection)
        if ticket.timestamp > now + MAX_FUTURE_TIMESTAMP_SECS {
            return Err(TicketError::FutureTimestamp {
                ticket_time: ticket.timestamp,
                current_time: now,
            });
        }

        // Reject stale timestamps
        if ticket.timestamp < now - MAX_STALE_TIMESTAMP_SECS {
            return Err(TicketError::StaleTimestamp {
                ticket_time: ticket.timestamp,
                current_time: now,
            });
        }

        Ok(())
    }

    /// Validate nonce is monotonically increasing.
    fn validate_nonce(
        &self,
        ticket: &PaymentTicket,
        channel_state: &ChannelState,
    ) -> Result<(), TicketError> {
        if ticket.nonce <= channel_state.last_nonce {
            return Err(TicketError::ReplayedNonce {
                ticket_nonce: ticket.nonce,
                last_nonce: channel_state.last_nonce,
            });
        }
        Ok(())
    }

    /// Validate amount is cumulative (non-decreasing).
    fn validate_amount(
        &self,
        ticket: &PaymentTicket,
        channel_state: &ChannelState,
    ) -> Result<(), TicketError> {
        // Amount must be >= last amount (cumulative)
        if ticket.amount_micros < channel_state.last_amount {
            return Err(TicketError::NonCumulativeAmount {
                ticket_amount: ticket.amount_micros,
                last_amount: channel_state.last_amount,
            });
        }

        // Amount must not exceed channel balance
        if ticket.amount_micros > channel_state.channel_balance {
            return Err(TicketError::InsufficientBalance {
                ticket_amount: ticket.amount_micros,
                channel_balance: channel_state.channel_balance,
            });
        }

        Ok(())
    }
}

#[async_trait]
impl TicketValidator for DefaultTicketValidator {
    async fn validate(
        &self,
        ticket: &PaymentTicket,
        payer_pubkey: &[u8; 32],
        channel_state: &ChannelState,
    ) -> Result<(), TicketError> {
        // 1. Verify signature (most important - cryptographic proof)
        self.verify_signature(ticket, payer_pubkey)?;

        // 2. Validate nonce (replay protection)
        self.validate_nonce(ticket, channel_state)?;

        // 3. Validate amount (cumulative and within balance)
        self.validate_amount(ticket, channel_state)?;

        // 4. Validate timestamp (staleness check)
        self.validate_timestamp(ticket)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::DefaultTicketSigner;
    use crate::ticket::TicketSigner;

    async fn create_test_signer_and_ticket() -> (DefaultTicketSigner, PaymentTicket) {
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);
        let ticket = signer
            .sign_ticket([1u8; 32], 1_000_000, 5)
            .await
            .expect("signing failed");
        (signer, ticket)
    }

    #[tokio::test]
    async fn test_valid_ticket() {
        let (signer, ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_signature() {
        let (signer, mut ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        // Create a ticket with corrupted signature
        let mut corrupted_sig = *ticket.signature();
        corrupted_sig[0] ^= 0xFF;
        ticket = PaymentTicket::new(
            ticket.channel_id,
            ticket.amount_micros,
            ticket.nonce,
            ticket.timestamp,
            corrupted_sig,
        );

        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(matches!(result, Err(TicketError::InvalidSignature)));
    }

    #[tokio::test]
    async fn test_wrong_public_key() {
        let (_, ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        // Use a different public key
        let wrong_pubkey = [99u8; 32];

        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &wrong_pubkey, &channel_state)
            .await;

        // Could be InvalidPublicKey or InvalidSignature depending on key validity
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_replayed_nonce() {
        let (signer, ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        // Set last_nonce >= ticket nonce
        let channel_state = ChannelState {
            last_nonce: 5, // Same as ticket
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(matches!(result, Err(TicketError::ReplayedNonce { .. })));
    }

    #[tokio::test]
    async fn test_non_cumulative_amount() {
        let (signer, ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        // Set last_amount > ticket amount
        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 2_000_000, // More than ticket's 1_000_000
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(matches!(
            result,
            Err(TicketError::NonCumulativeAmount { .. })
        ));
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let (signer, ticket) = create_test_signer_and_ticket().await;
        let validator = DefaultTicketValidator::new();

        // Set channel_balance < ticket amount
        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 500_000, // Less than ticket's 1_000_000
        };

        let result = validator
            .validate(&ticket, &signer.public_key(), &channel_state)
            .await;

        assert!(matches!(
            result,
            Err(TicketError::InsufficientBalance { .. })
        ));
    }

    #[tokio::test]
    async fn test_stale_timestamp() {
        let secret = [42u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret);
        let public_key = signing_key.verifying_key().to_bytes();

        // Create ticket manually with old timestamp
        let payload = crate::ticket::TicketPayload {
            channel_id: [1u8; 32],
            amount_micros: 1_000_000,
            nonce: 5,
        };
        let message = payload.to_bytes();
        let signature: ed25519_dalek::Signature =
            ed25519_dalek::Signer::sign(&signing_key, &message);

        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - MAX_STALE_TIMESTAMP_SECS
            - 10; // 10 seconds past stale threshold

        let ticket = PaymentTicket::new(
            payload.channel_id,
            payload.amount_micros,
            payload.nonce,
            old_timestamp,
            signature.to_bytes(),
        );

        let validator = DefaultTicketValidator::new();
        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &public_key, &channel_state)
            .await;

        assert!(matches!(result, Err(TicketError::StaleTimestamp { .. })));
    }

    #[tokio::test]
    async fn test_future_timestamp() {
        let secret = [42u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret);
        let public_key = signing_key.verifying_key().to_bytes();

        // Create ticket manually with future timestamp
        let payload = crate::ticket::TicketPayload {
            channel_id: [1u8; 32],
            amount_micros: 1_000_000,
            nonce: 5,
        };
        let message = payload.to_bytes();
        let signature: ed25519_dalek::Signature =
            ed25519_dalek::Signer::sign(&signing_key, &message);

        let future_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + MAX_FUTURE_TIMESTAMP_SECS
            + 10; // 10 seconds past future threshold

        let ticket = PaymentTicket::new(
            payload.channel_id,
            payload.amount_micros,
            payload.nonce,
            future_timestamp,
            signature.to_bytes(),
        );

        let validator = DefaultTicketValidator::new();
        let channel_state = ChannelState {
            last_nonce: 4,
            last_amount: 500_000,
            channel_balance: 10_000_000,
        };

        let result = validator
            .validate(&ticket, &public_key, &channel_state)
            .await;

        assert!(matches!(result, Err(TicketError::FutureTimestamp { .. })));
    }
}
