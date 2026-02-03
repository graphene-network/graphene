//! Ticket signing for payment authorization.
//!
//! Users sign tickets to authorize workers to claim payment from their channel.

use super::{PaymentTicket, TicketError, TicketPayload};
use async_trait::async_trait;
use ed25519_dalek::{Signature, Signer, SigningKey};
use std::time::{SystemTime, UNIX_EPOCH};

/// Trait for signing payment tickets.
///
/// Implementations handle key management and signature creation.
#[async_trait]
pub trait TicketSigner: Send + Sync {
    /// Sign a ticket payload and return a complete ticket.
    ///
    /// # Arguments
    /// * `channel_id` - Payment channel address
    /// * `amount_micros` - Cumulative amount to authorize
    /// * `nonce` - Ticket sequence number
    ///
    /// # Returns
    /// A signed `PaymentTicket` ready for transmission to workers.
    async fn sign_ticket(
        &self,
        channel_id: [u8; 32],
        amount_micros: u64,
        nonce: u64,
    ) -> Result<PaymentTicket, TicketError>;

    /// Get the public key bytes for this signer.
    fn public_key(&self) -> [u8; 32];
}

/// Default ticket signer using Ed25519.
pub struct DefaultTicketSigner {
    signing_key: SigningKey,
}

impl DefaultTicketSigner {
    /// Create a new signer from an Ed25519 signing key.
    pub fn new(signing_key: SigningKey) -> Self {
        Self { signing_key }
    }

    /// Create a signer from raw secret key bytes.
    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(secret),
        }
    }
}

impl std::fmt::Debug for DefaultTicketSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultTicketSigner")
            .field("public_key", &hex::encode(self.public_key()))
            .finish()
    }
}

#[async_trait]
impl TicketSigner for DefaultTicketSigner {
    async fn sign_ticket(
        &self,
        channel_id: [u8; 32],
        amount_micros: u64,
        nonce: u64,
    ) -> Result<PaymentTicket, TicketError> {
        let payload = TicketPayload {
            channel_id,
            amount_micros,
            nonce,
        };

        // Sign the 48-byte payload
        let message = payload.to_bytes();
        let signature: Signature = self.signing_key.sign(&message);

        // Get current timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TicketError::TimestampError)?
            .as_secs() as i64;

        Ok(PaymentTicket::new(
            channel_id,
            amount_micros,
            nonce,
            timestamp,
            signature.to_bytes(),
        ))
    }

    fn public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[tokio::test]
    async fn test_sign_ticket() {
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);

        let ticket = signer
            .sign_ticket([1u8; 32], 1_000_000, 1)
            .await
            .expect("signing failed");

        // Verify the ticket has correct values
        assert_eq!(ticket.channel_id, [1u8; 32]);
        assert_eq!(ticket.amount_micros, 1_000_000);
        assert_eq!(ticket.nonce, 1);
        assert!(ticket.timestamp > 0);

        // Verify the signature is valid
        let message = ticket.signed_message();
        let signature = Signature::from_bytes(ticket.signature());
        let verifying_key = signer.signing_key.verifying_key();

        assert!(verifying_key.verify(&message, &signature).is_ok());
    }

    #[tokio::test]
    async fn test_signature_covers_payload_only() {
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);

        // Sign two tickets with same payload but different timestamps
        let ticket1 = signer
            .sign_ticket([1u8; 32], 1_000_000, 1)
            .await
            .expect("signing failed");

        // Manually create ticket2 with same payload but different timestamp
        let ticket2 = PaymentTicket::new(
            ticket1.channel_id,
            ticket1.amount_micros,
            ticket1.nonce,
            ticket1.timestamp + 100, // Different timestamp
            *ticket1.signature(),    // Same signature
        );

        // Both should have the same signed message
        assert_eq!(ticket1.signed_message(), ticket2.signed_message());

        // Signature should still be valid for ticket2 (timestamp not signed)
        let message = ticket2.signed_message();
        let signature = Signature::from_bytes(ticket2.signature());
        let verifying_key = signer.signing_key.verifying_key();

        assert!(verifying_key.verify(&message, &signature).is_ok());
    }

    #[test]
    fn test_public_key() {
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);

        let expected = SigningKey::from_bytes(&secret).verifying_key().to_bytes();
        assert_eq!(signer.public_key(), expected);
    }
}
