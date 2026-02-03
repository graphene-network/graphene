//! Local channel state management for worker-side ticket verification.
//!
//! This module provides types and traits for tracking payment channel state
//! on the worker side. Workers maintain local state that mirrors on-chain
//! channel state with additional tracking for accepted tickets and settlement.
//!
//! # Architecture
//!
//! The worker tracks:
//! - **On-chain state**: Balance, Open/Closing status, dispute timeout
//! - **Local accepted state**: Sum of accepted ticket amounts, highest nonce
//! - **Settlement state**: Last confirmed on-chain settlement
//!
//! This separation allows workers to accept tickets immediately while
//! periodically syncing with on-chain state for settlement and dispute handling.

use async_trait::async_trait;

use super::{ChannelState, PaymentTicket, TicketError};

/// On-chain channel state (mirrors the Solana program's ChannelState enum).
///
/// Workers track this to know when to stop accepting tickets and when
/// to submit settlement proofs before the dispute window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnChainChannelState {
    /// Channel is open and accepting payments.
    #[default]
    Open,
    /// Channel is closing - dispute window is active.
    /// Workers should settle any outstanding tickets before timeout.
    Closing,
}

/// Local state tracked by a worker for a single payment channel.
///
/// This combines on-chain state (synced periodically) with local state
/// (updated on each accepted ticket). The worker uses this to:
/// - Validate incoming tickets against channel balance
/// - Track unsettled amounts for periodic settlement
/// - Detect disputes and channel closures
#[derive(Debug, Clone)]
pub struct ChannelLocalState {
    /// Payment channel PDA address (32-byte Solana pubkey).
    pub channel_id: [u8; 32],

    /// User's Ed25519 public key (channel depositor).
    pub user: [u8; 32],

    /// Worker's Ed25519 public key (this node).
    pub worker: [u8; 32],

    /// Last known on-chain balance (from chain sync).
    /// This is the total deposited amount, not available balance.
    pub on_chain_balance: u64,

    /// Sum of all accepted ticket amounts (cumulative).
    /// This increases as tickets are accepted.
    pub accepted_amount: u64,

    /// Last settlement amount confirmed on-chain.
    /// After settlement, this equals the on-chain `spent` field.
    pub last_settled_amount: u64,

    /// Highest nonce from accepted tickets.
    /// Used for replay protection and settlement.
    pub last_nonce: u64,

    /// Unix timestamp of last chain sync.
    pub last_sync: i64,

    /// Highest-value ticket received (for settlement proof).
    /// Workers submit this ticket's signature to claim on-chain.
    pub highest_ticket: Option<PaymentTicket>,

    /// Current on-chain state (Open or Closing).
    pub on_chain_state: OnChainChannelState,

    /// Unix timestamp when dispute window ends (0 if not in dispute).
    /// If > 0 and state is Closing, worker must settle before this time.
    pub dispute_timeout: i64,
}

impl ChannelLocalState {
    /// Returns the available balance that can still be spent.
    ///
    /// This is the on-chain balance minus what's already been accepted
    /// (but possibly not yet settled on-chain).
    #[inline]
    pub fn available_balance(&self) -> u64 {
        self.on_chain_balance.saturating_sub(self.accepted_amount)
    }

    /// Returns the amount accepted but not yet settled on-chain.
    ///
    /// Workers should periodically settle when this exceeds a threshold
    /// to reduce risk of losing funds if the channel closes.
    #[inline]
    pub fn unsettled_amount(&self) -> u64 {
        self.accepted_amount
            .saturating_sub(self.last_settled_amount)
    }

    /// Returns true if the channel is in closing state.
    #[inline]
    pub fn is_closing(&self) -> bool {
        self.on_chain_state == OnChainChannelState::Closing
    }

    /// Returns true if the channel is in an active dispute.
    ///
    /// When in dispute, the worker should prioritize settlement
    /// before the timeout expires.
    #[inline]
    pub fn in_dispute(&self) -> bool {
        self.dispute_timeout > 0
    }
}

/// Events from on-chain state changes that affect local channel state.
///
/// Workers receive these events from a chain watcher service and update
/// their local state accordingly.
#[derive(Debug, Clone)]
pub enum ChannelEvent {
    /// User topped up the channel with additional funds.
    TopUp {
        channel_id: [u8; 32],
        new_balance: u64,
    },

    /// User or worker initiated channel closure (dispute window started).
    DisputeInitiated {
        channel_id: [u8; 32],
        /// Unix timestamp when dispute window ends.
        timeout: i64,
    },

    /// Channel was closed (dispute resolved or timed out).
    /// Worker should remove this channel from tracking.
    ChannelClosed { channel_id: [u8; 32] },

    /// Settlement transaction confirmed on-chain.
    SettlementConfirmed {
        channel_id: [u8; 32],
        /// New cumulative settled amount.
        settled_amount: u64,
        /// Nonce from the settled ticket.
        new_nonce: u64,
    },

    /// Generic balance change (deposits, withdrawals).
    BalanceChanged {
        channel_id: [u8; 32],
        old_balance: u64,
        new_balance: u64,
    },
}

/// Result of attempting to accept a payment ticket.
#[derive(Debug, Clone)]
pub enum TicketAcceptResult {
    /// Ticket was accepted and state was updated.
    Accepted {
        /// New cumulative accepted amount for this channel.
        new_amount: u64,
        /// Current unsettled amount (may trigger settlement).
        unsettled: u64,
        /// True if unsettled amount exceeds threshold and settlement is recommended.
        needs_settlement: bool,
    },

    /// Ticket was rejected due to validation failure.
    Rejected(TicketError),
}

/// Errors that can occur during channel state operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelError {
    /// Channel not found in local state.
    #[error("channel not found: {channel_id:?}")]
    ChannelNotFound { channel_id: [u8; 32] },

    /// Channel is closing - not accepting new tickets.
    #[error("channel is closing")]
    ChannelClosing,

    /// Channel is in dispute - requires immediate settlement.
    #[error("channel in dispute, timeout at {timeout}")]
    ChannelInDispute { timeout: i64 },

    /// Ticket validation failed.
    #[error("ticket error: {0}")]
    TicketError(#[from] TicketError),

    /// Failed to sync with on-chain state.
    #[error("sync error: {0}")]
    SyncError(String),

    /// Local state doesn't match on-chain state.
    #[error("state mismatch: local={local}, chain={chain}")]
    StateMismatch { local: u64, chain: u64 },
}

/// Configuration for channel state management.
///
/// These thresholds control when workers trigger settlement and how
/// often they sync with on-chain state.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Maximum unsettled amount before triggering settlement (in microtokens).
    /// Default: 10,000,000 (10 USDC).
    pub max_unsettled_threshold: u64,

    /// How often to sync with on-chain state (seconds).
    /// Default: 600 (10 minutes).
    pub sync_interval_secs: u64,

    /// Maximum age of last sync before considering state stale (seconds).
    /// Stale channels may reject tickets until re-synced.
    /// Default: 1800 (30 minutes).
    pub max_staleness_secs: u64,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            max_unsettled_threshold: 10_000_000, // 10 USDC in microtokens
            sync_interval_secs: 600,             // 10 minutes
            max_staleness_secs: 1800,            // 30 minutes
        }
    }
}

/// Trait for managing local channel state.
///
/// Implementations handle storage (in-memory, SQLite, etc.) and provide
/// atomic operations for ticket acceptance and state updates.
///
/// # Thread Safety
///
/// All methods must be safe to call concurrently. Implementations should
/// use appropriate locking or atomic operations to prevent race conditions
/// when accepting tickets or handling events.
#[async_trait]
pub trait ChannelStateManager: Send + Sync {
    /// Get the local state for a channel.
    ///
    /// Returns `None` if the channel is not tracked.
    async fn get_channel(&self, channel_id: &[u8; 32]) -> Option<ChannelLocalState>;

    /// Insert or update channel state.
    ///
    /// Used when syncing from on-chain state or creating new channel tracking.
    async fn upsert_channel(&self, state: ChannelLocalState) -> Result<(), ChannelError>;

    /// Attempt to accept a ticket and update channel state.
    ///
    /// This validates the ticket against current channel state and atomically
    /// updates accepted_amount and last_nonce if valid.
    ///
    /// # Returns
    /// - `Ok(Accepted { ... })` - Ticket was accepted, state updated
    /// - `Ok(Rejected(error))` - Ticket failed validation, state unchanged
    /// - `Err(ChannelError)` - Operation failed (channel not found, closing, etc.)
    async fn accept_ticket(
        &self,
        channel_id: &[u8; 32],
        ticket: &PaymentTicket,
    ) -> Result<TicketAcceptResult, ChannelError>;

    /// Handle an on-chain event and update local state.
    ///
    /// Events include top-ups, disputes, settlements, and channel closures.
    async fn handle_event(&self, event: ChannelEvent) -> Result<(), ChannelError>;

    /// Get list of channels that need settlement.
    ///
    /// Returns channel IDs where unsettled_amount exceeds threshold or
    /// the channel is in dispute.
    async fn channels_needing_settlement(&self) -> Vec<[u8; 32]>;

    /// Mark a channel as settled up to the given amount and nonce.
    ///
    /// Called after a settlement transaction is confirmed on-chain.
    async fn mark_settled(
        &self,
        channel_id: &[u8; 32],
        amount: u64,
        nonce: u64,
    ) -> Result<(), ChannelError>;

    /// Remove a channel from tracking.
    ///
    /// Called when a channel is closed on-chain.
    async fn remove_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError>;

    /// Get the validation state for ticket verification.
    ///
    /// Returns the `ChannelState` type used by the ticket validator,
    /// extracted from the local channel state.
    async fn get_validation_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_state() -> ChannelLocalState {
        ChannelLocalState {
            channel_id: [1u8; 32],
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 10_000_000,   // 10 USDC
            accepted_amount: 3_000_000,     // 3 USDC accepted
            last_settled_amount: 1_000_000, // 1 USDC settled
            last_nonce: 5,
            last_sync: 1700000000,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        }
    }

    #[test]
    fn test_available_balance_normal() {
        let state = make_test_state();
        // 10M - 3M = 7M available
        assert_eq!(state.available_balance(), 7_000_000);
    }

    #[test]
    fn test_available_balance_fully_spent() {
        let mut state = make_test_state();
        state.accepted_amount = 10_000_000; // All spent
        assert_eq!(state.available_balance(), 0);
    }

    #[test]
    fn test_available_balance_overspent_saturates() {
        let mut state = make_test_state();
        state.accepted_amount = 15_000_000; // More than balance (shouldn't happen normally)
        assert_eq!(state.available_balance(), 0); // Saturates to 0
    }

    #[test]
    fn test_available_balance_zero_balance() {
        let mut state = make_test_state();
        state.on_chain_balance = 0;
        state.accepted_amount = 0;
        assert_eq!(state.available_balance(), 0);
    }

    #[test]
    fn test_unsettled_amount_normal() {
        let state = make_test_state();
        // 3M accepted - 1M settled = 2M unsettled
        assert_eq!(state.unsettled_amount(), 2_000_000);
    }

    #[test]
    fn test_unsettled_amount_fully_settled() {
        let mut state = make_test_state();
        state.last_settled_amount = 3_000_000; // All settled
        assert_eq!(state.unsettled_amount(), 0);
    }

    #[test]
    fn test_unsettled_amount_over_settled_saturates() {
        let mut state = make_test_state();
        state.last_settled_amount = 5_000_000; // More than accepted (shouldn't happen)
        assert_eq!(state.unsettled_amount(), 0); // Saturates to 0
    }

    #[test]
    fn test_unsettled_amount_nothing_accepted() {
        let mut state = make_test_state();
        state.accepted_amount = 0;
        state.last_settled_amount = 0;
        assert_eq!(state.unsettled_amount(), 0);
    }

    #[test]
    fn test_is_closing_open() {
        let state = make_test_state();
        assert!(!state.is_closing());
    }

    #[test]
    fn test_is_closing_closing() {
        let mut state = make_test_state();
        state.on_chain_state = OnChainChannelState::Closing;
        assert!(state.is_closing());
    }

    #[test]
    fn test_in_dispute_no_dispute() {
        let state = make_test_state();
        assert!(!state.in_dispute());
    }

    #[test]
    fn test_in_dispute_active_dispute() {
        let mut state = make_test_state();
        state.dispute_timeout = 1700003600; // 1 hour from now
        assert!(state.in_dispute());
    }

    #[test]
    fn test_in_dispute_zero_timeout() {
        let mut state = make_test_state();
        state.dispute_timeout = 0;
        assert!(!state.in_dispute());
    }

    #[test]
    fn test_channel_config_default_values() {
        let config = ChannelConfig::default();
        assert_eq!(config.max_unsettled_threshold, 10_000_000); // 10 USDC
        assert_eq!(config.sync_interval_secs, 600); // 10 minutes
        assert_eq!(config.max_staleness_secs, 1800); // 30 minutes
    }

    #[test]
    fn test_on_chain_channel_state_default() {
        let state = OnChainChannelState::default();
        assert_eq!(state, OnChainChannelState::Open);
    }

    #[test]
    fn test_channel_error_display() {
        let err = ChannelError::ChannelNotFound {
            channel_id: [0xAB; 32],
        };
        assert!(err.to_string().contains("channel not found"));

        let err = ChannelError::ChannelClosing;
        assert_eq!(err.to_string(), "channel is closing");

        let err = ChannelError::ChannelInDispute {
            timeout: 1700000000,
        };
        assert!(err.to_string().contains("1700000000"));

        let err = ChannelError::SyncError("network failure".into());
        assert!(err.to_string().contains("network failure"));

        let err = ChannelError::StateMismatch {
            local: 100,
            chain: 200,
        };
        assert!(err.to_string().contains("local=100"));
        assert!(err.to_string().contains("chain=200"));
    }

    #[test]
    fn test_ticket_error_into_channel_error() {
        let ticket_err = TicketError::InvalidSignature;
        let channel_err: ChannelError = ticket_err.into();
        assert!(matches!(channel_err, ChannelError::TicketError(_)));
    }
}
