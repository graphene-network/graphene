//! Default channel state manager implementation.
//!
//! Provides an in-memory implementation of `ChannelStateManager` for tracking
//! payment channel state on the worker side. This implementation uses
//! `Arc<RwLock<HashMap>>` for thread-safe concurrent access.
//!
//! # Example
//!
//! ```text
//! use graphene_node::ticket::{
//!     DefaultChannelStateManager, ChannelConfig, ChannelLocalState,
//!     ChannelStateManager, OnChainChannelState,
//! };
//!
//! let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());
//!
//! // Add a channel
//! let state = ChannelLocalState {
//!     channel_id: [1u8; 32],
//!     user: [2u8; 32],
//!     worker: [3u8; 32],
//!     on_chain_balance: 10_000_000,
//!     accepted_amount: 0,
//!     last_settled_amount: 0,
//!     last_nonce: 0,
//!     last_sync: 0,
//!     highest_ticket: None,
//!     on_chain_state: OnChainChannelState::Open,
//!     dispute_timeout: 0,
//! };
//! manager.upsert_channel(state).await?;
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{
    ChannelConfig, ChannelError, ChannelEvent, ChannelLocalState, ChannelState,
    ChannelStateManager, DefaultTicketValidator, OnChainChannelState, PaymentTicket,
    TicketAcceptResult, TicketValidator,
};

/// Default in-memory implementation of `ChannelStateManager`.
///
/// Uses `Arc<RwLock<HashMap>>` for thread-safe concurrent access to channel state.
/// This implementation is suitable for single-node deployments. For distributed
/// deployments, consider a persistent storage backend (e.g., SQLite, PostgreSQL).
pub struct DefaultChannelStateManager {
    /// Channel states indexed by channel_id.
    channels: Arc<RwLock<HashMap<[u8; 32], ChannelLocalState>>>,

    /// Configuration for settlement thresholds and sync intervals.
    config: ChannelConfig,

    /// Ticket validator for signature and business rule validation.
    validator: Arc<dyn TicketValidator>,
}

impl DefaultChannelStateManager {
    /// Create a new channel state manager with the given config and validator.
    pub fn new(config: ChannelConfig, validator: Arc<dyn TicketValidator>) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            config,
            validator,
        }
    }

    /// Create a new channel state manager with the default ticket validator.
    pub fn with_default_validator(config: ChannelConfig) -> Self {
        Self::new(config, Arc::new(DefaultTicketValidator::new()))
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }
}

#[async_trait]
impl ChannelStateManager for DefaultChannelStateManager {
    async fn get_channel(&self, channel_id: &[u8; 32]) -> Option<ChannelLocalState> {
        self.channels.read().await.get(channel_id).cloned()
    }

    async fn upsert_channel(&self, state: ChannelLocalState) -> Result<(), ChannelError> {
        self.channels.write().await.insert(state.channel_id, state);
        Ok(())
    }

    async fn accept_ticket(
        &self,
        channel_id: &[u8; 32],
        ticket: &PaymentTicket,
    ) -> Result<TicketAcceptResult, ChannelError> {
        // Get channel state (must hold write lock for atomic update)
        let mut channels = self.channels.write().await;

        let channel = channels
            .get_mut(channel_id)
            .ok_or(ChannelError::ChannelNotFound {
                channel_id: *channel_id,
            })?;

        // Check channel is not closing or in dispute
        if channel.is_closing() {
            return Err(ChannelError::ChannelClosing);
        }

        if channel.in_dispute() {
            return Err(ChannelError::ChannelInDispute {
                timeout: channel.dispute_timeout,
            });
        }

        // Build validation state from local channel state
        let validation_state = ChannelState {
            last_nonce: channel.last_nonce,
            last_amount: channel.accepted_amount,
            channel_balance: channel.on_chain_balance,
        };

        // Validate the ticket
        if let Err(e) = self
            .validator
            .validate(ticket, &channel.user, &validation_state)
            .await
        {
            return Ok(TicketAcceptResult::Rejected(e));
        }

        // Update local state
        channel.last_nonce = ticket.nonce;
        channel.accepted_amount = ticket.amount_micros;

        // Store as highest ticket if amount is higher
        let should_update_highest = channel
            .highest_ticket
            .as_ref()
            .map(|t| ticket.amount_micros > t.amount_micros)
            .unwrap_or(true);

        if should_update_highest {
            channel.highest_ticket = Some(ticket.clone());
        }

        // Calculate unsettled amount and check if settlement is needed
        let unsettled = channel.unsettled_amount();
        let needs_settlement = unsettled > self.config.max_unsettled_threshold;

        Ok(TicketAcceptResult::Accepted {
            new_amount: channel.accepted_amount,
            unsettled,
            needs_settlement,
        })
    }

    async fn handle_event(&self, event: ChannelEvent) -> Result<(), ChannelError> {
        match event {
            ChannelEvent::TopUp {
                channel_id,
                new_balance,
            } => {
                let mut channels = self.channels.write().await;
                if let Some(channel) = channels.get_mut(&channel_id) {
                    channel.on_chain_balance = new_balance;
                }
                // If channel not found, ignore (may have been removed)
                Ok(())
            }

            ChannelEvent::DisputeInitiated {
                channel_id,
                timeout,
            } => {
                let mut channels = self.channels.write().await;
                if let Some(channel) = channels.get_mut(&channel_id) {
                    channel.dispute_timeout = timeout;
                    channel.on_chain_state = OnChainChannelState::Closing;
                }
                Ok(())
            }

            ChannelEvent::ChannelClosed { channel_id } => {
                self.channels.write().await.remove(&channel_id);
                Ok(())
            }

            ChannelEvent::SettlementConfirmed {
                channel_id,
                settled_amount,
                new_nonce,
            } => {
                let mut channels = self.channels.write().await;
                if let Some(channel) = channels.get_mut(&channel_id) {
                    channel.last_settled_amount = settled_amount;
                    // Only update nonce if the chain nonce is higher
                    // (shouldn't happen, but be defensive)
                    if new_nonce > channel.last_nonce {
                        channel.last_nonce = new_nonce;
                    }
                }
                Ok(())
            }

            ChannelEvent::BalanceChanged {
                channel_id,
                new_balance,
                ..
            } => {
                let mut channels = self.channels.write().await;
                if let Some(channel) = channels.get_mut(&channel_id) {
                    channel.on_chain_balance = new_balance;
                }
                Ok(())
            }
        }
    }

    async fn channels_needing_settlement(&self) -> Vec<[u8; 32]> {
        self.channels
            .read()
            .await
            .iter()
            .filter(|(_, channel)| {
                // Needs settlement if unsettled exceeds threshold or in dispute
                channel.unsettled_amount() > self.config.max_unsettled_threshold
                    || channel.in_dispute()
            })
            .map(|(id, _)| *id)
            .collect()
    }

    async fn mark_settled(
        &self,
        channel_id: &[u8; 32],
        amount: u64,
        nonce: u64,
    ) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;

        let channel = channels
            .get_mut(channel_id)
            .ok_or(ChannelError::ChannelNotFound {
                channel_id: *channel_id,
            })?;

        channel.last_settled_amount = amount;
        channel.last_nonce = nonce;

        Ok(())
    }

    async fn remove_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError> {
        self.channels.write().await.remove(channel_id);
        Ok(())
    }

    async fn get_validation_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState> {
        self.channels
            .read()
            .await
            .get(channel_id)
            .map(|channel| ChannelState {
                last_nonce: channel.last_nonce,
                last_amount: channel.accepted_amount,
                channel_balance: channel.on_chain_balance,
            })
    }
}

// ============================================================================
// Mock Implementation
// ============================================================================

/// Configurable mock behavior for channel state manager.
#[derive(Debug, Clone, Default)]
pub enum MockChannelBehavior {
    /// Normal happy path behavior.
    #[default]
    HappyPath,

    /// Always return channel not found.
    ChannelNotFound,

    /// Always return channel closing error.
    ChannelClosing,

    /// Mark all channels as needing settlement.
    AlwaysNeedsSettlement,
}

/// Mock channel state manager for testing.
///
/// Tracks accepted tickets and events for test assertions.
pub struct MockChannelStateManager {
    behavior: MockChannelBehavior,
    channels: Arc<RwLock<HashMap<[u8; 32], ChannelLocalState>>>,
    accepted_tickets: Arc<RwLock<Vec<PaymentTicket>>>,
    events_received: Arc<RwLock<Vec<ChannelEvent>>>,
    config: ChannelConfig,
}

impl Default for MockChannelStateManager {
    fn default() -> Self {
        Self::new(MockChannelBehavior::HappyPath)
    }
}

impl MockChannelStateManager {
    /// Create a new mock with the specified behavior.
    pub fn new(behavior: MockChannelBehavior) -> Self {
        Self {
            behavior,
            channels: Arc::new(RwLock::new(HashMap::new())),
            accepted_tickets: Arc::new(RwLock::new(Vec::new())),
            events_received: Arc::new(RwLock::new(Vec::new())),
            config: ChannelConfig::default(),
        }
    }

    /// Create a mock that always accepts tickets (happy path).
    pub fn happy_path() -> Self {
        Self::new(MockChannelBehavior::HappyPath)
    }

    /// Create a mock that always returns channel not found.
    pub fn channel_not_found() -> Self {
        Self::new(MockChannelBehavior::ChannelNotFound)
    }

    /// Create a mock that always returns channel closing error.
    pub fn channel_closing() -> Self {
        Self::new(MockChannelBehavior::ChannelClosing)
    }

    /// Get tickets that were accepted.
    pub async fn accepted_tickets(&self) -> Vec<PaymentTicket> {
        self.accepted_tickets.read().await.clone()
    }

    /// Get events that were received.
    pub async fn events_received(&self) -> Vec<ChannelEvent> {
        self.events_received.read().await.clone()
    }

    /// Inject a channel state for testing.
    pub async fn inject_channel(&self, state: ChannelLocalState) {
        self.channels.write().await.insert(state.channel_id, state);
    }

    /// Clear all state (for test reset).
    pub async fn clear(&self) {
        self.channels.write().await.clear();
        self.accepted_tickets.write().await.clear();
        self.events_received.write().await.clear();
    }
}

#[async_trait]
impl ChannelStateManager for MockChannelStateManager {
    async fn get_channel(&self, channel_id: &[u8; 32]) -> Option<ChannelLocalState> {
        match self.behavior {
            MockChannelBehavior::ChannelNotFound => None,
            _ => self.channels.read().await.get(channel_id).cloned(),
        }
    }

    async fn upsert_channel(&self, state: ChannelLocalState) -> Result<(), ChannelError> {
        self.channels.write().await.insert(state.channel_id, state);
        Ok(())
    }

    async fn accept_ticket(
        &self,
        channel_id: &[u8; 32],
        ticket: &PaymentTicket,
    ) -> Result<TicketAcceptResult, ChannelError> {
        match self.behavior {
            MockChannelBehavior::ChannelNotFound => Err(ChannelError::ChannelNotFound {
                channel_id: *channel_id,
            }),

            MockChannelBehavior::ChannelClosing => Err(ChannelError::ChannelClosing),

            MockChannelBehavior::HappyPath | MockChannelBehavior::AlwaysNeedsSettlement => {
                // Track accepted ticket
                self.accepted_tickets.write().await.push(ticket.clone());

                // Update channel if exists
                let mut channels = self.channels.write().await;
                if let Some(channel) = channels.get_mut(channel_id) {
                    channel.last_nonce = ticket.nonce;
                    channel.accepted_amount = ticket.amount_micros;
                }

                let needs_settlement =
                    matches!(self.behavior, MockChannelBehavior::AlwaysNeedsSettlement);

                Ok(TicketAcceptResult::Accepted {
                    new_amount: ticket.amount_micros,
                    unsettled: ticket.amount_micros,
                    needs_settlement,
                })
            }
        }
    }

    async fn handle_event(&self, event: ChannelEvent) -> Result<(), ChannelError> {
        self.events_received.write().await.push(event.clone());

        // Also apply the event to state for realistic behavior
        match event {
            ChannelEvent::TopUp {
                channel_id,
                new_balance,
            } => {
                if let Some(channel) = self.channels.write().await.get_mut(&channel_id) {
                    channel.on_chain_balance = new_balance;
                }
            }
            ChannelEvent::DisputeInitiated {
                channel_id,
                timeout,
            } => {
                if let Some(channel) = self.channels.write().await.get_mut(&channel_id) {
                    channel.dispute_timeout = timeout;
                    channel.on_chain_state = OnChainChannelState::Closing;
                }
            }
            ChannelEvent::ChannelClosed { channel_id } => {
                self.channels.write().await.remove(&channel_id);
            }
            ChannelEvent::SettlementConfirmed {
                channel_id,
                settled_amount,
                new_nonce,
            } => {
                if let Some(channel) = self.channels.write().await.get_mut(&channel_id) {
                    channel.last_settled_amount = settled_amount;
                    if new_nonce > channel.last_nonce {
                        channel.last_nonce = new_nonce;
                    }
                }
            }
            ChannelEvent::BalanceChanged {
                channel_id,
                new_balance,
                ..
            } => {
                if let Some(channel) = self.channels.write().await.get_mut(&channel_id) {
                    channel.on_chain_balance = new_balance;
                }
            }
        }

        Ok(())
    }

    async fn channels_needing_settlement(&self) -> Vec<[u8; 32]> {
        match self.behavior {
            MockChannelBehavior::AlwaysNeedsSettlement => {
                self.channels.read().await.keys().copied().collect()
            }
            _ => self
                .channels
                .read()
                .await
                .iter()
                .filter(|(_, channel)| {
                    channel.unsettled_amount() > self.config.max_unsettled_threshold
                        || channel.in_dispute()
                })
                .map(|(id, _)| *id)
                .collect(),
        }
    }

    async fn mark_settled(
        &self,
        channel_id: &[u8; 32],
        amount: u64,
        nonce: u64,
    ) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;

        let channel = channels
            .get_mut(channel_id)
            .ok_or(ChannelError::ChannelNotFound {
                channel_id: *channel_id,
            })?;

        channel.last_settled_amount = amount;
        channel.last_nonce = nonce;

        Ok(())
    }

    async fn remove_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError> {
        self.channels.write().await.remove(channel_id);
        Ok(())
    }

    async fn get_validation_state(&self, channel_id: &[u8; 32]) -> Option<ChannelState> {
        match self.behavior {
            MockChannelBehavior::ChannelNotFound => None,
            _ => self
                .channels
                .read()
                .await
                .get(channel_id)
                .map(|channel| ChannelState {
                    last_nonce: channel.last_nonce,
                    last_amount: channel.accepted_amount,
                    channel_balance: channel.on_chain_balance,
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::{DefaultTicketSigner, MockTicketValidator, TicketSigner};

    fn make_test_channel_state(channel_id: [u8; 32]) -> ChannelLocalState {
        ChannelLocalState {
            channel_id,
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 10_000_000, // 10 USDC
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 1700000000,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        }
    }

    // ========================================================================
    // DefaultChannelStateManager Tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_and_get_channel() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);

        manager.upsert_channel(state.clone()).await.unwrap();

        let retrieved = manager.get_channel(&channel_id).await;
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.channel_id, channel_id);
        assert_eq!(retrieved.on_chain_balance, 10_000_000);
    }

    #[tokio::test]
    async fn test_get_channel_not_found() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [99u8; 32];
        let retrieved = manager.get_channel(&channel_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_accept_ticket_valid() {
        // Use mock validator that always accepts
        let validator = Arc::new(MockTicketValidator::always_valid());
        let manager = DefaultChannelStateManager::new(ChannelConfig::default(), validator);

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.user = [42u8; 32]; // Match signer key
        manager.upsert_channel(state).await.unwrap();

        // Create a ticket
        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = manager.accept_ticket(&channel_id, &ticket).await.unwrap();

        match result {
            TicketAcceptResult::Accepted {
                new_amount,
                unsettled,
                needs_settlement,
            } => {
                assert_eq!(new_amount, 1_000_000);
                assert_eq!(unsettled, 1_000_000);
                assert!(!needs_settlement); // Below threshold
            }
            TicketAcceptResult::Rejected(_) => panic!("Expected ticket to be accepted"),
        }

        // Verify state was updated
        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.accepted_amount, 1_000_000);
        assert_eq!(channel.last_nonce, 1);
        assert!(channel.highest_ticket.is_some());
    }

    #[tokio::test]
    async fn test_accept_ticket_rejected_invalid_signature() {
        // Use mock validator that rejects with invalid signature
        let validator = Arc::new(MockTicketValidator::always_invalid_signature());
        let manager = DefaultChannelStateManager::new(ChannelConfig::default(), validator);

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = manager.accept_ticket(&channel_id, &ticket).await.unwrap();

        match result {
            TicketAcceptResult::Rejected(e) => {
                assert!(matches!(e, crate::ticket::TicketError::InvalidSignature));
            }
            TicketAcceptResult::Accepted { .. } => panic!("Expected ticket to be rejected"),
        }

        // Verify state was NOT updated
        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.accepted_amount, 0);
        assert_eq!(channel.last_nonce, 0);
    }

    #[tokio::test]
    async fn test_accept_ticket_channel_not_found() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [99u8; 32];
        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = manager.accept_ticket(&channel_id, &ticket).await;

        assert!(matches!(result, Err(ChannelError::ChannelNotFound { .. })));
    }

    #[tokio::test]
    async fn test_accept_ticket_channel_closing() {
        let validator = Arc::new(MockTicketValidator::always_valid());
        let manager = DefaultChannelStateManager::new(ChannelConfig::default(), validator);

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.on_chain_state = OnChainChannelState::Closing;
        manager.upsert_channel(state).await.unwrap();

        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = manager.accept_ticket(&channel_id, &ticket).await;

        assert!(matches!(result, Err(ChannelError::ChannelClosing)));
    }

    #[tokio::test]
    async fn test_accept_ticket_channel_in_dispute() {
        let validator = Arc::new(MockTicketValidator::always_valid());
        let manager = DefaultChannelStateManager::new(ChannelConfig::default(), validator);

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.dispute_timeout = 1700003600; // Active dispute
        manager.upsert_channel(state).await.unwrap();

        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = manager.accept_ticket(&channel_id, &ticket).await;

        assert!(matches!(result, Err(ChannelError::ChannelInDispute { .. })));
    }

    #[tokio::test]
    async fn test_handle_event_top_up() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        manager
            .handle_event(ChannelEvent::TopUp {
                channel_id,
                new_balance: 20_000_000,
            })
            .await
            .unwrap();

        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.on_chain_balance, 20_000_000);
    }

    #[tokio::test]
    async fn test_handle_event_settlement_confirmed() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.accepted_amount = 5_000_000;
        state.last_nonce = 10;
        manager.upsert_channel(state).await.unwrap();

        manager
            .handle_event(ChannelEvent::SettlementConfirmed {
                channel_id,
                settled_amount: 5_000_000,
                new_nonce: 10,
            })
            .await
            .unwrap();

        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.last_settled_amount, 5_000_000);
        assert_eq!(channel.unsettled_amount(), 0);
    }

    #[tokio::test]
    async fn test_handle_event_channel_closed() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        assert!(manager.get_channel(&channel_id).await.is_some());

        manager
            .handle_event(ChannelEvent::ChannelClosed { channel_id })
            .await
            .unwrap();

        assert!(manager.get_channel(&channel_id).await.is_none());
    }

    #[tokio::test]
    async fn test_handle_event_dispute_initiated() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        manager
            .handle_event(ChannelEvent::DisputeInitiated {
                channel_id,
                timeout: 1700003600,
            })
            .await
            .unwrap();

        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.dispute_timeout, 1700003600);
        assert_eq!(channel.on_chain_state, OnChainChannelState::Closing);
        assert!(channel.is_closing());
        assert!(channel.in_dispute());
    }

    #[tokio::test]
    async fn test_channels_needing_settlement_threshold() {
        let config = ChannelConfig {
            max_unsettled_threshold: 1_000_000, // 1 USDC threshold
            ..Default::default()
        };
        let validator = Arc::new(MockTicketValidator::always_valid());
        let manager = DefaultChannelStateManager::new(config, validator);

        // Channel 1: Under threshold
        let channel_id_1 = [1u8; 32];
        let mut state_1 = make_test_channel_state(channel_id_1);
        state_1.accepted_amount = 500_000; // 0.5 USDC unsettled
        manager.upsert_channel(state_1).await.unwrap();

        // Channel 2: Over threshold
        let channel_id_2 = [2u8; 32];
        let mut state_2 = make_test_channel_state(channel_id_2);
        state_2.accepted_amount = 2_000_000; // 2 USDC unsettled
        manager.upsert_channel(state_2).await.unwrap();

        let needing_settlement = manager.channels_needing_settlement().await;

        assert_eq!(needing_settlement.len(), 1);
        assert!(needing_settlement.contains(&channel_id_2));
    }

    #[tokio::test]
    async fn test_channels_needing_settlement_dispute() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        // Channel in dispute (even with no unsettled amount)
        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.dispute_timeout = 1700003600;
        manager.upsert_channel(state).await.unwrap();

        let needing_settlement = manager.channels_needing_settlement().await;

        assert_eq!(needing_settlement.len(), 1);
        assert!(needing_settlement.contains(&channel_id));
    }

    #[tokio::test]
    async fn test_mark_settled() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.accepted_amount = 5_000_000;
        state.last_nonce = 10;
        manager.upsert_channel(state).await.unwrap();

        manager
            .mark_settled(&channel_id, 5_000_000, 10)
            .await
            .unwrap();

        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.last_settled_amount, 5_000_000);
        assert_eq!(channel.unsettled_amount(), 0);
    }

    #[tokio::test]
    async fn test_mark_settled_not_found() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [99u8; 32];
        let result = manager.mark_settled(&channel_id, 5_000_000, 10).await;

        assert!(matches!(result, Err(ChannelError::ChannelNotFound { .. })));
    }

    #[tokio::test]
    async fn test_remove_channel() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        assert!(manager.get_channel(&channel_id).await.is_some());

        manager.remove_channel(&channel_id).await.unwrap();

        assert!(manager.get_channel(&channel_id).await.is_none());
    }

    #[tokio::test]
    async fn test_get_validation_state() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.accepted_amount = 3_000_000;
        state.last_nonce = 5;
        manager.upsert_channel(state).await.unwrap();

        let validation_state = manager.get_validation_state(&channel_id).await.unwrap();

        assert_eq!(validation_state.last_nonce, 5);
        assert_eq!(validation_state.last_amount, 3_000_000);
        assert_eq!(validation_state.channel_balance, 10_000_000);
    }

    #[tokio::test]
    async fn test_get_validation_state_not_found() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        let channel_id = [99u8; 32];
        let validation_state = manager.get_validation_state(&channel_id).await;

        assert!(validation_state.is_none());
    }

    // ========================================================================
    // MockChannelStateManager Tests
    // ========================================================================

    #[tokio::test]
    async fn test_mock_happy_path() {
        let mock = MockChannelStateManager::happy_path();

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        mock.inject_channel(state).await;

        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = mock.accept_ticket(&channel_id, &ticket).await.unwrap();

        assert!(matches!(result, TicketAcceptResult::Accepted { .. }));

        // Verify tracking
        let accepted = mock.accepted_tickets().await;
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].amount_micros, 1_000_000);
    }

    #[tokio::test]
    async fn test_mock_channel_not_found() {
        let mock = MockChannelStateManager::channel_not_found();

        let channel_id = [1u8; 32];
        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = mock.accept_ticket(&channel_id, &ticket).await;

        assert!(matches!(result, Err(ChannelError::ChannelNotFound { .. })));
    }

    #[tokio::test]
    async fn test_mock_channel_closing() {
        let mock = MockChannelStateManager::channel_closing();

        let channel_id = [1u8; 32];
        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);

        let result = mock.accept_ticket(&channel_id, &ticket).await;

        assert!(matches!(result, Err(ChannelError::ChannelClosing)));
    }

    #[tokio::test]
    async fn test_mock_events_tracked() {
        let mock = MockChannelStateManager::happy_path();

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        mock.inject_channel(state).await;

        mock.handle_event(ChannelEvent::TopUp {
            channel_id,
            new_balance: 20_000_000,
        })
        .await
        .unwrap();

        let events = mock.events_received().await;
        assert_eq!(events.len(), 1);

        // Also verify state was updated
        let channel = mock.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.on_chain_balance, 20_000_000);
    }

    #[tokio::test]
    async fn test_mock_always_needs_settlement() {
        let mock = MockChannelStateManager::new(MockChannelBehavior::AlwaysNeedsSettlement);

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        mock.inject_channel(state).await;

        let needing = mock.channels_needing_settlement().await;
        assert_eq!(needing.len(), 1);

        let ticket = PaymentTicket::new(channel_id, 100, 1, 1700000000, [0u8; 64]);
        let result = mock.accept_ticket(&channel_id, &ticket).await.unwrap();

        match result {
            TicketAcceptResult::Accepted {
                needs_settlement, ..
            } => {
                assert!(needs_settlement);
            }
            _ => panic!("Expected accepted"),
        }
    }

    #[tokio::test]
    async fn test_mock_clear() {
        let mock = MockChannelStateManager::happy_path();

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        mock.inject_channel(state).await;

        let ticket = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
        mock.accept_ticket(&channel_id, &ticket).await.unwrap();

        assert!(!mock.accepted_tickets().await.is_empty());
        assert!(mock.get_channel(&channel_id).await.is_some());

        mock.clear().await;

        assert!(mock.accepted_tickets().await.is_empty());
        assert!(mock.get_channel(&channel_id).await.is_none());
    }

    // ========================================================================
    // Integration test with real validator
    // ========================================================================

    #[tokio::test]
    async fn test_accept_ticket_with_real_validator() {
        let manager = DefaultChannelStateManager::with_default_validator(ChannelConfig::default());

        // Create signer and get public key
        let secret = [42u8; 32];
        let signer = DefaultTicketSigner::from_bytes(&secret);
        let user_pubkey = signer.public_key();

        // Create channel with matching user pubkey
        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.user = user_pubkey;
        manager.upsert_channel(state).await.unwrap();

        // Sign a valid ticket
        let ticket = signer
            .sign_ticket(channel_id, 1_000_000, 1)
            .await
            .expect("signing failed");

        // Accept the ticket
        let result = manager.accept_ticket(&channel_id, &ticket).await.unwrap();

        match result {
            TicketAcceptResult::Accepted {
                new_amount,
                unsettled,
                needs_settlement,
            } => {
                assert_eq!(new_amount, 1_000_000);
                assert_eq!(unsettled, 1_000_000);
                assert!(!needs_settlement);
            }
            TicketAcceptResult::Rejected(e) => panic!("Unexpected rejection: {:?}", e),
        }

        // Verify channel state was updated
        let channel = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(channel.accepted_amount, 1_000_000);
        assert_eq!(channel.last_nonce, 1);
        assert!(channel.highest_ticket.is_some());
    }

    #[tokio::test]
    async fn test_accept_higher_ticket_updates_highest() {
        let validator = Arc::new(MockTicketValidator::always_valid());
        let manager = DefaultChannelStateManager::new(ChannelConfig::default(), validator);

        let channel_id = [1u8; 32];
        let state = make_test_channel_state(channel_id);
        manager.upsert_channel(state).await.unwrap();

        // First ticket
        let ticket1 = PaymentTicket::new(channel_id, 1_000_000, 1, 1700000000, [0u8; 64]);
        manager.accept_ticket(&channel_id, &ticket1).await.unwrap();

        // Second ticket with higher amount
        let ticket2 = PaymentTicket::new(channel_id, 2_000_000, 2, 1700000001, [0u8; 64]);
        manager.accept_ticket(&channel_id, &ticket2).await.unwrap();

        let channel = manager.get_channel(&channel_id).await.unwrap();
        let highest = channel.highest_ticket.unwrap();
        assert_eq!(highest.amount_micros, 2_000_000);
        assert_eq!(highest.nonce, 2);
    }
}
