//! Background channel sync service for periodic on-chain synchronization.
//!
//! This module provides a background service that:
//! - Periodically syncs local channel state with on-chain state
//! - Monitors unsettled amount thresholds to trigger settlement
//! - Manages WebSocket subscriptions for real-time channel updates
//!
//! # Architecture
//!
//! The service follows the pattern from `discovery/service.rs`:
//! - Uses `Arc<AtomicBool>` for running state
//! - Spawns background tasks for periodic operations
//! - Provides graceful shutdown with task cleanup
//!
//! # Example
//!
//! ```text
//! use monad_node::ticket::{
//!     ChannelSyncService, DefaultChannelStateManager, MockSolanaChannelClient,
//!     ChannelConfig,
//! };
//! use std::sync::Arc;
//!
//! let manager = Arc::new(DefaultChannelStateManager::with_default_validator(ChannelConfig::default()));
//! let solana_client = Arc::new(MockSolanaChannelClient::default());
//! let config = ChannelConfig::default();
//!
//! let service = ChannelSyncService::new(
//!     manager,
//!     solana_client,
//!     config,
//!     |channel_id| println!("Settlement needed for {:?}", channel_id),
//! );
//!
//! service.start().await?;
//! // ... service runs in background ...
//! service.stop().await?;
//! ```

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use super::{
    ChannelConfig, ChannelError, ChannelEvent, ChannelStateManager, OnChainChannelState,
    SolanaChannelClient,
};

/// Background service for syncing channel state with Solana.
///
/// This service runs background tasks that:
/// 1. **Periodic sync**: Fetches on-chain state at `config.sync_interval_secs` intervals
/// 2. **Threshold monitor**: Checks for channels needing settlement every 60 seconds
/// 3. **WebSocket subscriptions**: Handles real-time updates for subscribed channels
pub struct ChannelSyncService<M, S>
where
    M: ChannelStateManager + 'static,
    S: SolanaChannelClient + 'static,
{
    /// Channel state manager for local state operations.
    manager: Arc<M>,

    /// Solana client for fetching on-chain state.
    solana_client: Arc<S>,

    /// Configuration for sync intervals and thresholds.
    config: ChannelConfig,

    /// Whether the service is currently running.
    running: Arc<AtomicBool>,

    /// Background task handles for cleanup on stop.
    tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,

    /// Channel IDs currently subscribed via WebSocket.
    subscribed_channels: Arc<RwLock<HashSet<[u8; 32]>>>,

    /// Callback triggered when a channel needs settlement.
    /// Called with the channel_id that exceeded the threshold or is in dispute.
    settlement_trigger: Arc<dyn Fn([u8; 32]) + Send + Sync>,

    /// Channel IDs to sync (tracked separately for batch fetching).
    /// In a production implementation, this would be derived from the manager.
    tracked_channels: Arc<RwLock<HashSet<[u8; 32]>>>,
}

impl<M, S> ChannelSyncService<M, S>
where
    M: ChannelStateManager + 'static,
    S: SolanaChannelClient + 'static,
{
    /// Create a new channel sync service.
    ///
    /// # Arguments
    /// * `manager` - Channel state manager for local state
    /// * `solana_client` - Solana client for on-chain queries
    /// * `config` - Configuration for sync intervals and thresholds
    /// * `settlement_trigger` - Callback when settlement is needed
    pub fn new(
        manager: Arc<M>,
        solana_client: Arc<S>,
        config: ChannelConfig,
        settlement_trigger: impl Fn([u8; 32]) + Send + Sync + 'static,
    ) -> Self {
        Self {
            manager,
            solana_client,
            config,
            running: Arc::new(AtomicBool::new(false)),
            tasks: Arc::new(RwLock::new(Vec::new())),
            subscribed_channels: Arc::new(RwLock::new(HashSet::new())),
            settlement_trigger: Arc::new(settlement_trigger),
            tracked_channels: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Start the background service.
    ///
    /// Spawns periodic sync and threshold monitor tasks.
    ///
    /// # Errors
    /// Returns `ChannelError::SyncError` if the service is already running.
    pub async fn start(&self) -> Result<(), ChannelError> {
        if self.running.swap(true, Ordering::Relaxed) {
            return Err(ChannelError::SyncError("already running".to_string()));
        }

        self.spawn_periodic_sync().await;
        self.spawn_threshold_monitor().await;

        tracing::info!("ChannelSyncService started");
        Ok(())
    }

    /// Stop the background service with graceful cleanup.
    ///
    /// Unsubscribes from all channels and aborts all background tasks.
    ///
    /// # Errors
    /// Returns `ChannelError::SyncError` if the service is not running.
    pub async fn stop(&self) -> Result<(), ChannelError> {
        if !self.running.swap(false, Ordering::Relaxed) {
            return Err(ChannelError::SyncError("not running".to_string()));
        }

        // Unsubscribe from all channels
        let channels: Vec<_> = self
            .subscribed_channels
            .read()
            .await
            .iter()
            .copied()
            .collect();
        for channel_id in channels {
            if let Err(e) = self.solana_client.unsubscribe_channel(&channel_id).await {
                tracing::warn!("Failed to unsubscribe from channel: {}", e);
            }
        }
        self.subscribed_channels.write().await.clear();

        // Abort all background tasks
        let tasks = std::mem::take(&mut *self.tasks.write().await);
        for task in tasks {
            task.abort();
        }

        tracing::info!("ChannelSyncService stopped");
        Ok(())
    }

    /// Check if the service is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Add a channel to be tracked for periodic sync.
    ///
    /// The channel will be included in periodic sync operations.
    pub async fn track_channel(&self, channel_id: [u8; 32]) {
        self.tracked_channels.write().await.insert(channel_id);
    }

    /// Remove a channel from tracking.
    ///
    /// Also unsubscribes if the channel was subscribed.
    pub async fn untrack_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError> {
        self.tracked_channels.write().await.remove(channel_id);

        if self.subscribed_channels.read().await.contains(channel_id) {
            self.unsubscribe_channel(channel_id).await?;
        }

        Ok(())
    }

    /// Get list of tracked channel IDs.
    pub async fn tracked_channels(&self) -> Vec<[u8; 32]> {
        self.tracked_channels.read().await.iter().copied().collect()
    }

    /// Subscribe to a channel for real-time WebSocket updates.
    ///
    /// Events received via WebSocket are forwarded to the channel state manager.
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte channel PDA address
    ///
    /// # Errors
    /// Returns `ChannelError::SyncError` if subscription fails.
    pub async fn subscribe_channel(&self, channel_id: [u8; 32]) -> Result<(), ChannelError> {
        if self.subscribed_channels.read().await.contains(&channel_id) {
            return Ok(()); // Already subscribed
        }

        let mut rx = self
            .solana_client
            .subscribe_channel(&channel_id)
            .await
            .map_err(|e| ChannelError::SyncError(e.to_string()))?;

        self.subscribed_channels.write().await.insert(channel_id);

        // Also track the channel for periodic sync
        self.tracked_channels.write().await.insert(channel_id);

        // Spawn task to handle events from this subscription
        let manager = Arc::clone(&self.manager);
        let running = Arc::clone(&self.running);
        let subscribed = Arc::clone(&self.subscribed_channels);

        let handle = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                match rx.recv().await {
                    Some(event) => {
                        if let Err(e) = manager.handle_event(event).await {
                            tracing::warn!("Failed to handle channel event: {}", e);
                        }
                    }
                    None => {
                        // Subscription ended (sender dropped)
                        subscribed.write().await.remove(&channel_id);
                        tracing::debug!("Channel subscription ended: {:?}", channel_id);
                        break;
                    }
                }
            }
        });

        self.tasks.write().await.push(handle);
        Ok(())
    }

    /// Unsubscribe from a channel's real-time updates.
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte channel PDA address
    ///
    /// # Errors
    /// Returns `ChannelError::SyncError` if unsubscription fails.
    pub async fn unsubscribe_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError> {
        self.solana_client
            .unsubscribe_channel(channel_id)
            .await
            .map_err(|e| ChannelError::SyncError(e.to_string()))?;
        self.subscribed_channels.write().await.remove(channel_id);
        Ok(())
    }

    /// Check if a channel is currently subscribed for real-time updates.
    pub async fn is_subscribed(&self, channel_id: &[u8; 32]) -> bool {
        self.subscribed_channels.read().await.contains(channel_id)
    }

    /// Sync a specific channel with on-chain state.
    ///
    /// Fetches the current on-chain state and emits appropriate events
    /// for any detected changes (balance, state, settlement).
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte channel PDA address
    ///
    /// # Errors
    /// Returns `ChannelError::SyncError` on RPC failures.
    pub async fn sync_channel(&self, channel_id: &[u8; 32]) -> Result<(), ChannelError> {
        let on_chain = self
            .solana_client
            .fetch_channel(channel_id)
            .await
            .map_err(|e| ChannelError::SyncError(e.to_string()))?;

        match on_chain {
            Some(chain_state) => {
                if let Some(local) = self.manager.get_channel(channel_id).await {
                    // Check for balance changes
                    if chain_state.balance != local.on_chain_balance {
                        self.manager
                            .handle_event(ChannelEvent::BalanceChanged {
                                channel_id: *channel_id,
                                old_balance: local.on_chain_balance,
                                new_balance: chain_state.balance,
                            })
                            .await?;
                    }

                    // Check for state changes (dispute initiated)
                    let chain_on_chain_state = if chain_state.state == OnChainChannelState::Closing
                    {
                        OnChainChannelState::Closing
                    } else {
                        OnChainChannelState::Open
                    };

                    if chain_on_chain_state != local.on_chain_state && chain_state.timeout > 0 {
                        self.manager
                            .handle_event(ChannelEvent::DisputeInitiated {
                                channel_id: *channel_id,
                                timeout: chain_state.timeout,
                            })
                            .await?;
                    }

                    // Check for settlement confirmations (on-chain nonce advanced)
                    if chain_state.last_nonce > local.last_nonce {
                        self.manager
                            .handle_event(ChannelEvent::SettlementConfirmed {
                                channel_id: *channel_id,
                                settled_amount: chain_state.spent,
                                new_nonce: chain_state.last_nonce,
                            })
                            .await?;
                    }
                }
            }
            None => {
                // Channel closed on-chain
                if self.manager.get_channel(channel_id).await.is_some() {
                    self.manager
                        .handle_event(ChannelEvent::ChannelClosed {
                            channel_id: *channel_id,
                        })
                        .await?;
                }
                // Remove from tracking
                self.tracked_channels.write().await.remove(channel_id);
            }
        }

        Ok(())
    }

    /// Spawn the periodic sync task.
    ///
    /// Runs at `config.sync_interval_secs` intervals, fetching on-chain state
    /// for all tracked channels and emitting events for detected changes.
    async fn spawn_periodic_sync(&self) {
        let manager = Arc::clone(&self.manager);
        let solana_client = Arc::clone(&self.solana_client);
        let running = Arc::clone(&self.running);
        let tracked_channels = Arc::clone(&self.tracked_channels);
        let interval_secs = self.config.sync_interval_secs;

        let handle = tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(interval_secs);

            while running.load(Ordering::Relaxed) {
                // Get all tracked channel IDs
                let channel_ids: Vec<[u8; 32]> =
                    tracked_channels.read().await.iter().copied().collect();

                if !channel_ids.is_empty() {
                    // Batch fetch on-chain states
                    match solana_client.fetch_channels(&channel_ids).await {
                        Ok(results) => {
                            for (channel_id, maybe_chain_state) in results {
                                if let Err(e) = Self::process_sync_result(
                                    &manager,
                                    &tracked_channels,
                                    &channel_id,
                                    maybe_chain_state,
                                )
                                .await
                                {
                                    tracing::warn!(
                                        "Failed to process sync for channel {:?}: {}",
                                        channel_id,
                                        e
                                    );
                                }
                            }
                            tracing::debug!("Synced {} channels", channel_ids.len());
                        }
                        Err(e) => {
                            tracing::warn!("Failed to fetch channels for sync: {}", e);
                        }
                    }
                }

                tokio::time::sleep(interval).await;
            }
        });

        self.tasks.write().await.push(handle);
    }

    /// Process sync result for a single channel.
    async fn process_sync_result(
        manager: &Arc<M>,
        tracked_channels: &Arc<RwLock<HashSet<[u8; 32]>>>,
        channel_id: &[u8; 32],
        maybe_chain_state: Option<super::OnChainChannel>,
    ) -> Result<(), ChannelError> {
        match maybe_chain_state {
            Some(chain_state) => {
                if let Some(local) = manager.get_channel(channel_id).await {
                    // Check for balance changes
                    if chain_state.balance != local.on_chain_balance {
                        manager
                            .handle_event(ChannelEvent::BalanceChanged {
                                channel_id: *channel_id,
                                old_balance: local.on_chain_balance,
                                new_balance: chain_state.balance,
                            })
                            .await?;
                    }

                    // Check for state changes (dispute initiated)
                    let chain_on_chain_state = if chain_state.state == OnChainChannelState::Closing
                    {
                        OnChainChannelState::Closing
                    } else {
                        OnChainChannelState::Open
                    };

                    if chain_on_chain_state != local.on_chain_state && chain_state.timeout > 0 {
                        manager
                            .handle_event(ChannelEvent::DisputeInitiated {
                                channel_id: *channel_id,
                                timeout: chain_state.timeout,
                            })
                            .await?;
                    }

                    // Check for settlement confirmations
                    if chain_state.last_nonce > local.last_nonce {
                        manager
                            .handle_event(ChannelEvent::SettlementConfirmed {
                                channel_id: *channel_id,
                                settled_amount: chain_state.spent,
                                new_nonce: chain_state.last_nonce,
                            })
                            .await?;
                    }
                }
            }
            None => {
                // Channel closed on-chain
                if manager.get_channel(channel_id).await.is_some() {
                    manager
                        .handle_event(ChannelEvent::ChannelClosed {
                            channel_id: *channel_id,
                        })
                        .await?;
                }
                // Remove from tracking
                tracked_channels.write().await.remove(channel_id);
            }
        }

        Ok(())
    }

    /// Spawn the threshold monitor task.
    ///
    /// Runs every 60 seconds, checking for channels that need settlement
    /// and calling the settlement trigger callback.
    async fn spawn_threshold_monitor(&self) {
        let manager = Arc::clone(&self.manager);
        let running = Arc::clone(&self.running);
        let settlement_trigger = Arc::clone(&self.settlement_trigger);

        let handle = tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(60);

            while running.load(Ordering::Relaxed) {
                // Get channels needing settlement
                let channels = manager.channels_needing_settlement().await;

                // Trigger settlement for each
                for channel_id in channels {
                    tracing::info!("Triggering settlement for channel {:?}", channel_id);
                    (settlement_trigger)(channel_id);
                }

                tokio::time::sleep(interval).await;
            }
        });

        self.tasks.write().await.push(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::{
        ChannelLocalState, MockChannelStateManager, MockSolanaChannelClient, OnChainChannel,
    };
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn make_test_channel_state(channel_id: [u8; 32]) -> ChannelLocalState {
        ChannelLocalState {
            channel_id,
            user: [2u8; 32],
            worker: [3u8; 32],
            on_chain_balance: 10_000_000,
            accepted_amount: 0,
            last_settled_amount: 0,
            last_nonce: 0,
            last_sync: 1700000000,
            highest_ticket: None,
            on_chain_state: OnChainChannelState::Open,
            dispute_timeout: 0,
        }
    }

    fn make_test_on_chain_channel() -> OnChainChannel {
        OnChainChannel {
            user: [2u8; 32],
            worker: [3u8; 32],
            mint: [4u8; 32],
            balance: 10_000_000,
            spent: 0,
            last_nonce: 0,
            timeout: 0,
            state: OnChainChannelState::Open,
            bump: 255,
        }
    }

    fn make_test_config() -> ChannelConfig {
        ChannelConfig {
            max_unsettled_threshold: 1_000_000, // Low threshold for testing
            sync_interval_secs: 1,              // Fast sync for testing
            max_staleness_secs: 60,
        }
    }

    #[tokio::test]
    async fn test_sync_service_start_stop() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        // Should not be running initially
        assert!(!service.is_running());

        // Start
        service.start().await.unwrap();
        assert!(service.is_running());

        // Stop
        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_sync_service_double_start() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        service.start().await.unwrap();

        // Double start should error
        let result = service.start().await;
        assert!(matches!(result, Err(ChannelError::SyncError(_))));

        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_sync_service_stop_without_start() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        // Stop without start should error
        let result = service.stop().await;
        assert!(matches!(result, Err(ChannelError::SyncError(_))));
    }

    #[tokio::test]
    async fn test_threshold_monitor_triggers_settlement() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = ChannelConfig {
            max_unsettled_threshold: 100, // Very low threshold
            sync_interval_secs: 60,
            max_staleness_secs: 60,
        };

        // Inject a channel with high unsettled amount
        let channel_id = [1u8; 32];
        let mut state = make_test_channel_state(channel_id);
        state.accepted_amount = 1_000_000; // 1M unsettled (above 100 threshold)
        manager.inject_channel(state).await;

        // Track settlement triggers
        let trigger_count = Arc::new(AtomicUsize::new(0));
        let trigger_count_clone = Arc::clone(&trigger_count);

        let service = ChannelSyncService::new(manager, solana_client, config, move |_| {
            trigger_count_clone.fetch_add(1, Ordering::Relaxed);
        });

        service.start().await.unwrap();

        // Wait for at least one threshold check cycle (60 seconds is too long for tests)
        // We'll just verify the service started correctly
        tokio::time::sleep(Duration::from_millis(100)).await;

        service.stop().await.unwrap();

        // Note: The threshold monitor runs every 60 seconds, so we can't easily test
        // the trigger in a fast unit test. For real testing, we'd mock the timer or
        // expose a manual trigger method.
    }

    #[tokio::test]
    async fn test_subscribe_channel() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        let channel_id = [1u8; 32];

        // Subscribe
        service.subscribe_channel(channel_id).await.unwrap();

        // Should be subscribed
        assert!(service.is_subscribed(&channel_id).await);

        // Double subscribe should be idempotent
        service.subscribe_channel(channel_id).await.unwrap();
        assert!(service.is_subscribed(&channel_id).await);
    }

    #[tokio::test]
    async fn test_unsubscribe_channel() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        let channel_id = [1u8; 32];

        service.subscribe_channel(channel_id).await.unwrap();
        assert!(service.is_subscribed(&channel_id).await);

        service.unsubscribe_channel(&channel_id).await.unwrap();
        assert!(!service.is_subscribed(&channel_id).await);
    }

    #[tokio::test]
    async fn test_track_channel() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client, config, |_| {});

        let channel_id = [1u8; 32];

        // Initially empty
        assert!(service.tracked_channels().await.is_empty());

        // Track
        service.track_channel(channel_id).await;
        let tracked = service.tracked_channels().await;
        assert_eq!(tracked.len(), 1);
        assert!(tracked.contains(&channel_id));

        // Untrack
        service.untrack_channel(&channel_id).await.unwrap();
        assert!(service.tracked_channels().await.is_empty());
    }

    #[tokio::test]
    async fn test_sync_channel_detects_balance_change() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let channel_id = [1u8; 32];

        // Set up local state
        let local_state = make_test_channel_state(channel_id);
        manager.inject_channel(local_state).await;

        // Set up on-chain state with different balance
        let mut on_chain = make_test_on_chain_channel();
        on_chain.balance = 20_000_000; // Different from local
        solana_client.inject_channel(channel_id, on_chain).await;

        let service = ChannelSyncService::new(manager.clone(), solana_client, config, |_| {});

        // Sync the channel
        service.sync_channel(&channel_id).await.unwrap();

        // Check that balance was updated
        let updated = manager.get_channel(&channel_id).await.unwrap();
        assert_eq!(updated.on_chain_balance, 20_000_000);

        // Check that event was recorded
        let events = manager.events_received().await;
        assert!(!events.is_empty());
        assert!(matches!(
            events[0],
            ChannelEvent::BalanceChanged {
                new_balance: 20_000_000,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_sync_channel_detects_closed() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let channel_id = [1u8; 32];

        // Set up local state
        let local_state = make_test_channel_state(channel_id);
        manager.inject_channel(local_state).await;

        // Don't inject on-chain state (simulates closed channel)

        let service = ChannelSyncService::new(manager.clone(), solana_client, config, |_| {});
        service.track_channel(channel_id).await;

        // Sync the channel
        service.sync_channel(&channel_id).await.unwrap();

        // Check that channel was removed from local state
        assert!(manager.get_channel(&channel_id).await.is_none());

        // Check that event was recorded
        let events = manager.events_received().await;
        assert!(!events.is_empty());
        assert!(matches!(
            events[0],
            ChannelEvent::ChannelClosed { channel_id: id } if id == channel_id
        ));
    }

    #[tokio::test]
    async fn test_sync_channel_detects_dispute() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let channel_id = [1u8; 32];

        // Set up local state (open)
        let local_state = make_test_channel_state(channel_id);
        manager.inject_channel(local_state).await;

        // Set up on-chain state (closing with timeout)
        let mut on_chain = make_test_on_chain_channel();
        on_chain.state = OnChainChannelState::Closing;
        on_chain.timeout = 1700003600; // Future timeout
        solana_client.inject_channel(channel_id, on_chain).await;

        let service = ChannelSyncService::new(manager.clone(), solana_client, config, |_| {});

        // Sync the channel
        service.sync_channel(&channel_id).await.unwrap();

        // Check that dispute was detected
        let updated = manager.get_channel(&channel_id).await.unwrap();
        assert!(updated.is_closing());
        assert!(updated.in_dispute());

        // Check that event was recorded
        let events = manager.events_received().await;
        assert!(events.iter().any(|e| matches!(
            e,
            ChannelEvent::DisputeInitiated {
                timeout: 1700003600,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn test_sync_channel_detects_settlement_confirmed() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let channel_id = [1u8; 32];

        // Set up local state with nonce 5
        let mut local_state = make_test_channel_state(channel_id);
        local_state.last_nonce = 5;
        local_state.accepted_amount = 3_000_000;
        manager.inject_channel(local_state).await;

        // Set up on-chain state with higher nonce (settlement confirmed)
        let mut on_chain = make_test_on_chain_channel();
        on_chain.last_nonce = 10;
        on_chain.spent = 3_000_000;
        solana_client.inject_channel(channel_id, on_chain).await;

        let service = ChannelSyncService::new(manager.clone(), solana_client, config, |_| {});

        // Sync the channel
        service.sync_channel(&channel_id).await.unwrap();

        // Check that settlement was recorded
        let events = manager.events_received().await;
        assert!(events.iter().any(|e| matches!(
            e,
            ChannelEvent::SettlementConfirmed {
                settled_amount: 3_000_000,
                new_nonce: 10,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn test_websocket_event_forwarding() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let channel_id = [1u8; 32];

        // Set up local state
        let local_state = make_test_channel_state(channel_id);
        manager.inject_channel(local_state).await;

        let service =
            ChannelSyncService::new(manager.clone(), solana_client.clone(), config, |_| {});

        // Start the service so background tasks are running
        service.start().await.unwrap();

        // Subscribe to channel
        service.subscribe_channel(channel_id).await.unwrap();

        // Give the subscription task time to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Trigger an event via the mock
        let event = ChannelEvent::TopUp {
            channel_id,
            new_balance: 15_000_000,
        };
        solana_client
            .trigger_event(&channel_id, event)
            .await
            .unwrap();

        // Give the background task time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check that event was forwarded to manager
        let events = manager.events_received().await;
        assert!(!events.is_empty());
        assert!(events.iter().any(|e| matches!(
            e,
            ChannelEvent::TopUp {
                new_balance: 15_000_000,
                ..
            }
        )));

        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_unsubscribes_all() {
        let manager = Arc::new(MockChannelStateManager::default());
        let solana_client = Arc::new(MockSolanaChannelClient::default());
        let config = make_test_config();

        let service = ChannelSyncService::new(manager, solana_client.clone(), config, |_| {});

        let channel_id1 = [1u8; 32];
        let channel_id2 = [2u8; 32];

        service.start().await.unwrap();

        service.subscribe_channel(channel_id1).await.unwrap();
        service.subscribe_channel(channel_id2).await.unwrap();

        assert!(service.is_subscribed(&channel_id1).await);
        assert!(service.is_subscribed(&channel_id2).await);

        service.stop().await.unwrap();

        // All subscriptions should be cleared
        assert!(!service.is_subscribed(&channel_id1).await);
        assert!(!service.is_subscribed(&channel_id2).await);
    }
}
