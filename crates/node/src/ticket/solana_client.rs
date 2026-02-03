//! Solana client for fetching on-chain payment channel state.
//!
//! This module provides traits and implementations for interacting with
//! Solana to fetch channel state, derive PDAs, and subscribe to account changes.
//!
//! # Architecture
//!
//! The [`SolanaChannelClient`] trait abstracts Solana RPC interactions, enabling:
//! - Real implementations using `solana-client`
//! - Mock implementations for testing without network access
//!
//! # On-Chain Data Format
//!
//! The on-chain PaymentChannel account is 138 bytes:
//! - 8 bytes: Anchor discriminator
//! - 32 bytes: user pubkey
//! - 32 bytes: worker pubkey
//! - 32 bytes: mint pubkey
//! - 8 bytes: balance (u64 LE)
//! - 8 bytes: spent (u64 LE)
//! - 8 bytes: last_nonce (u64 LE)
//! - 8 bytes: timeout (i64 LE)
//! - 1 byte: state (0=Open, 1=Closing)
//! - 1 byte: bump

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::channel_state::{ChannelEvent, OnChainChannelState};

/// Errors that can occur during Solana client operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SolanaClientError {
    /// RPC communication error.
    #[error("RPC error: {0}")]
    RpcError(String),

    /// Account not found on-chain.
    #[error("account not found: {0}")]
    AccountNotFound(String),

    /// Account data has unexpected size.
    #[error("invalid account data: expected {expected} bytes, got {actual}")]
    InvalidAccountData { expected: usize, actual: usize },

    /// Failed to parse account data.
    #[error("parse error: {0}")]
    ParseError(String),

    /// WebSocket subscription error.
    #[error("subscription error: {0}")]
    SubscriptionError(String),

    /// WebSocket connection was closed.
    #[error("connection closed")]
    ConnectionClosed,
}

/// Parsed on-chain channel data (138 bytes total with discriminator).
///
/// Matches `programs/graphene/src/state/channel.rs` PaymentChannel.
#[derive(Debug, Clone)]
pub struct OnChainChannel {
    /// User who opened the channel (depositor).
    pub user: [u8; 32],
    /// Worker receiving payments.
    pub worker: [u8; 32],
    /// Token mint for this channel.
    pub mint: [u8; 32],
    /// Total deposited balance in channel.
    pub balance: u64,
    /// Cumulative amount spent (claimed by worker).
    pub spent: u64,
    /// Last settled nonce (monotonically increasing).
    pub last_nonce: u64,
    /// Unix timestamp when dispute window ends (0 if not closing).
    pub timeout: i64,
    /// Current channel state (Open or Closing).
    pub state: OnChainChannelState,
    /// PDA bump seed.
    pub bump: u8,
}

impl OnChainChannel {
    /// Total account data size including 8-byte Anchor discriminator.
    pub const LEN: usize = 138;

    /// Anchor discriminator size.
    const DISCRIMINATOR_LEN: usize = 8;

    /// Parse from account data bytes (138 bytes including discriminator).
    ///
    /// # Arguments
    /// * `data` - Raw account data from Solana RPC
    ///
    /// # Errors
    /// Returns `InvalidAccountData` if the data is not exactly 138 bytes,
    /// or `ParseError` if the state byte is invalid.
    pub fn from_bytes(data: &[u8]) -> Result<Self, SolanaClientError> {
        if data.len() != Self::LEN {
            return Err(SolanaClientError::InvalidAccountData {
                expected: Self::LEN,
                actual: data.len(),
            });
        }

        // Skip 8-byte discriminator
        let data = &data[Self::DISCRIMINATOR_LEN..];

        // Parse user pubkey (bytes 0-32)
        let user: [u8; 32] = data[0..32]
            .try_into()
            .map_err(|_| SolanaClientError::ParseError("failed to parse user".into()))?;

        // Parse worker pubkey (bytes 32-64)
        let worker: [u8; 32] = data[32..64]
            .try_into()
            .map_err(|_| SolanaClientError::ParseError("failed to parse worker".into()))?;

        // Parse mint pubkey (bytes 64-96)
        let mint: [u8; 32] = data[64..96]
            .try_into()
            .map_err(|_| SolanaClientError::ParseError("failed to parse mint".into()))?;

        // Parse balance (bytes 96-104, little-endian u64)
        let balance = u64::from_le_bytes(
            data[96..104]
                .try_into()
                .map_err(|_| SolanaClientError::ParseError("failed to parse balance".into()))?,
        );

        // Parse spent (bytes 104-112, little-endian u64)
        let spent = u64::from_le_bytes(
            data[104..112]
                .try_into()
                .map_err(|_| SolanaClientError::ParseError("failed to parse spent".into()))?,
        );

        // Parse last_nonce (bytes 112-120, little-endian u64)
        let last_nonce = u64::from_le_bytes(
            data[112..120]
                .try_into()
                .map_err(|_| SolanaClientError::ParseError("failed to parse last_nonce".into()))?,
        );

        // Parse timeout (bytes 120-128, little-endian i64)
        let timeout = i64::from_le_bytes(
            data[120..128]
                .try_into()
                .map_err(|_| SolanaClientError::ParseError("failed to parse timeout".into()))?,
        );

        // Parse state (byte 128, 0=Open, 1=Closing)
        let state = match data[128] {
            0 => OnChainChannelState::Open,
            1 => OnChainChannelState::Closing,
            other => {
                return Err(SolanaClientError::ParseError(format!(
                    "invalid channel state byte: {other}"
                )))
            }
        };

        // Parse bump (byte 129)
        let bump = data[129];

        Ok(Self {
            user,
            worker,
            mint,
            balance,
            spent,
            last_nonce,
            timeout,
            state,
            bump,
        })
    }

    /// Serialize to bytes (for testing).
    #[cfg(test)]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(Self::LEN);

        // Discriminator (8 bytes of zeros for testing)
        data.extend_from_slice(&[0u8; 8]);

        // Fields
        data.extend_from_slice(&self.user);
        data.extend_from_slice(&self.worker);
        data.extend_from_slice(&self.mint);
        data.extend_from_slice(&self.balance.to_le_bytes());
        data.extend_from_slice(&self.spent.to_le_bytes());
        data.extend_from_slice(&self.last_nonce.to_le_bytes());
        data.extend_from_slice(&self.timeout.to_le_bytes());
        data.push(match self.state {
            OnChainChannelState::Open => 0,
            OnChainChannelState::Closing => 1,
        });
        data.push(self.bump);

        data
    }
}

/// Trait for interacting with Solana to fetch payment channel state.
///
/// Implementations provide RPC access to the Graphene program's channel accounts.
/// The trait is designed to be mockable for testing without network access.
#[async_trait]
pub trait SolanaChannelClient: Send + Sync {
    /// Fetch channel state from Solana by channel PDA address.
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte PDA address of the channel account
    ///
    /// # Returns
    /// - `Ok(Some(channel))` - Channel exists and was parsed successfully
    /// - `Ok(None)` - Channel account does not exist
    /// - `Err(...)` - RPC or parsing error
    async fn fetch_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<Option<OnChainChannel>, SolanaClientError>;

    /// Fetch multiple channels in a single RPC batch call.
    ///
    /// More efficient than individual `fetch_channel` calls when fetching many channels.
    ///
    /// # Arguments
    /// * `channel_ids` - Array of 32-byte PDA addresses
    ///
    /// # Returns
    /// Vector of (channel_id, Option<channel>) pairs. Missing channels return None.
    async fn fetch_channels(
        &self,
        channel_ids: &[[u8; 32]],
    ) -> Result<Vec<([u8; 32], Option<OnChainChannel>)>, SolanaClientError>;

    /// Derive the channel PDA from user and worker public keys.
    ///
    /// Seeds: `[b"channel", user, worker]`
    ///
    /// # Arguments
    /// * `user` - User's 32-byte public key
    /// * `worker` - Worker's 32-byte public key
    ///
    /// # Returns
    /// The 32-byte PDA address for this channel
    fn derive_channel_pda(&self, user: &[u8; 32], worker: &[u8; 32]) -> [u8; 32];

    /// Subscribe to channel account changes via WebSocket.
    ///
    /// Returns a receiver that will emit [`ChannelEvent`]s when the
    /// on-chain state changes (deposits, disputes, settlements, closures).
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte PDA address to watch
    ///
    /// # Returns
    /// An mpsc receiver for channel events. The sender is held internally
    /// and events are pushed when account changes are detected.
    async fn subscribe_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<tokio::sync::mpsc::Receiver<ChannelEvent>, SolanaClientError>;

    /// Unsubscribe from channel account changes.
    ///
    /// Stops the WebSocket subscription and closes the event receiver.
    ///
    /// # Arguments
    /// * `channel_id` - The 32-byte PDA address to stop watching
    async fn unsubscribe_channel(&self, channel_id: &[u8; 32]) -> Result<(), SolanaClientError>;
}

/// Handle for managing a subscription internally.
struct SubscriptionHandle {
    /// Sender for pushing events to the subscriber.
    #[allow(dead_code)]
    sender: tokio::sync::mpsc::Sender<ChannelEvent>,
    /// Abort handle for the subscription task (if applicable).
    #[allow(dead_code)]
    abort_handle: Option<tokio::task::AbortHandle>,
}

/// Real Solana channel client using `solana-client`.
///
/// Connects to a Solana RPC endpoint and optionally a WebSocket endpoint
/// for account subscriptions.
pub struct DefaultSolanaChannelClient {
    /// Solana RPC URL (e.g., "https://api.devnet.solana.com").
    rpc_url: String,
    /// Solana WebSocket URL (e.g., "wss://api.devnet.solana.com").
    ws_url: String,
    /// Graphene program ID (32-byte public key).
    program_id: [u8; 32],
    /// Active subscriptions by channel ID.
    subscriptions: Arc<RwLock<HashMap<[u8; 32], SubscriptionHandle>>>,
}

impl DefaultSolanaChannelClient {
    /// Create a new client with the specified endpoints and program ID.
    ///
    /// # Arguments
    /// * `rpc_url` - HTTP(S) RPC endpoint URL
    /// * `ws_url` - WebSocket endpoint URL for subscriptions
    /// * `program_id` - The 32-byte Graphene program public key
    pub fn new(rpc_url: String, ws_url: String, program_id: [u8; 32]) -> Self {
        Self {
            rpc_url,
            ws_url,
            program_id,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the RPC URL.
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Get the WebSocket URL.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Get the program ID.
    pub fn program_id(&self) -> &[u8; 32] {
        &self.program_id
    }
}

#[async_trait]
impl SolanaChannelClient for DefaultSolanaChannelClient {
    async fn fetch_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<Option<OnChainChannel>, SolanaClientError> {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::pubkey::Pubkey;

        let client = RpcClient::new(&self.rpc_url);
        let pubkey = Pubkey::new_from_array(*channel_id);

        // Use spawn_blocking since RpcClient is synchronous
        let result = tokio::task::spawn_blocking(move || client.get_account(&pubkey))
            .await
            .map_err(|e| SolanaClientError::RpcError(format!("task join error: {e}")))?;

        match result {
            Ok(account) => {
                let channel = OnChainChannel::from_bytes(&account.data)?;
                Ok(Some(channel))
            }
            Err(e) => {
                // Check if it's a "not found" error
                let err_string = e.to_string();
                if err_string.contains("AccountNotFound")
                    || err_string.contains("could not find account")
                {
                    Ok(None)
                } else {
                    Err(SolanaClientError::RpcError(err_string))
                }
            }
        }
    }

    async fn fetch_channels(
        &self,
        channel_ids: &[[u8; 32]],
    ) -> Result<Vec<([u8; 32], Option<OnChainChannel>)>, SolanaClientError> {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::pubkey::Pubkey;

        if channel_ids.is_empty() {
            return Ok(Vec::new());
        }

        let client = RpcClient::new(&self.rpc_url);
        let pubkeys: Vec<Pubkey> = channel_ids
            .iter()
            .map(|id| Pubkey::new_from_array(*id))
            .collect();
        let ids = channel_ids.to_vec();

        // Use spawn_blocking since RpcClient is synchronous
        let result = tokio::task::spawn_blocking(move || client.get_multiple_accounts(&pubkeys))
            .await
            .map_err(|e| SolanaClientError::RpcError(format!("task join error: {e}")))?
            .map_err(|e| SolanaClientError::RpcError(e.to_string()))?;

        let mut channels = Vec::with_capacity(ids.len());
        for (id, maybe_account) in ids.into_iter().zip(result.into_iter()) {
            let parsed = match maybe_account {
                Some(account) => Some(OnChainChannel::from_bytes(&account.data)?),
                None => None,
            };
            channels.push((id, parsed));
        }

        Ok(channels)
    }

    fn derive_channel_pda(&self, user: &[u8; 32], worker: &[u8; 32]) -> [u8; 32] {
        use solana_sdk::pubkey::Pubkey;

        let program_id = Pubkey::new_from_array(self.program_id);
        let seeds: &[&[u8]] = &[b"channel", user, worker];

        let (pda, _bump) = Pubkey::find_program_address(seeds, &program_id);
        pda.to_bytes()
    }

    async fn subscribe_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<tokio::sync::mpsc::Receiver<ChannelEvent>, SolanaClientError> {
        // Create a channel for events
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        // Store the subscription handle
        // Note: Full WebSocket subscription implementation would require
        // spawning a background task with solana_client::nonblocking::pubsub_client
        let handle = SubscriptionHandle {
            sender: tx,
            abort_handle: None,
        };

        self.subscriptions.write().await.insert(*channel_id, handle);

        // TODO(#XXX): Implement actual WebSocket subscription using
        // solana_client::nonblocking::pubsub_client::PubsubClient::account_subscribe
        // For now, return the receiver - events would be pushed by a background task

        Ok(rx)
    }

    async fn unsubscribe_channel(&self, channel_id: &[u8; 32]) -> Result<(), SolanaClientError> {
        let mut subs = self.subscriptions.write().await;
        if let Some(handle) = subs.remove(channel_id) {
            // Abort the subscription task if it exists
            if let Some(abort) = handle.abort_handle {
                abort.abort();
            }
            // Dropping the sender will close the receiver
        }
        Ok(())
    }
}

// ============================================================================
// Mock Implementation
// ============================================================================

/// Configurable behavior for the mock Solana client.
#[derive(Clone, Default)]
pub enum MockSolanaBehavior {
    /// Normal operation - fetch channels as configured.
    #[default]
    HappyPath,
    /// Return RPC errors for all fetch operations.
    RpcError,
    /// Return None for all channels (account not found).
    AccountNotFound,
    /// Return invalid data errors when parsing.
    InvalidData,
}

/// Mock Solana channel client for testing.
///
/// Allows injecting channel state and triggering events without network access.
pub struct MockSolanaChannelClient {
    /// Configurable behavior for operations.
    behavior: MockSolanaBehavior,
    /// Mock channel state storage.
    channels: Arc<RwLock<HashMap<[u8; 32], OnChainChannel>>>,
    /// Event senders for subscribed channels (to trigger events in tests).
    event_senders: Arc<RwLock<HashMap<[u8; 32], tokio::sync::mpsc::Sender<ChannelEvent>>>>,
    /// Mock program ID.
    program_id: [u8; 32],
}

impl Default for MockSolanaChannelClient {
    fn default() -> Self {
        Self::new(MockSolanaBehavior::HappyPath)
    }
}

impl MockSolanaChannelClient {
    /// Create a new mock client with the specified behavior.
    pub fn new(behavior: MockSolanaBehavior) -> Self {
        Self {
            behavior,
            channels: Arc::new(RwLock::new(HashMap::new())),
            event_senders: Arc::new(RwLock::new(HashMap::new())),
            program_id: [0u8; 32],
        }
    }

    /// Create a mock that always succeeds.
    pub fn happy_path() -> Self {
        Self::new(MockSolanaBehavior::HappyPath)
    }

    /// Create a mock that always returns RPC errors.
    pub fn rpc_error() -> Self {
        Self::new(MockSolanaBehavior::RpcError)
    }

    /// Set the mock program ID.
    pub fn with_program_id(mut self, program_id: [u8; 32]) -> Self {
        self.program_id = program_id;
        self
    }

    /// Inject a channel into the mock storage.
    pub async fn inject_channel(&self, channel_id: [u8; 32], channel: OnChainChannel) {
        self.channels.write().await.insert(channel_id, channel);
    }

    /// Remove a channel from the mock storage.
    pub async fn remove_channel(&self, channel_id: &[u8; 32]) {
        self.channels.write().await.remove(channel_id);
    }

    /// Trigger an event for a subscribed channel (for testing event handlers).
    pub async fn trigger_event(
        &self,
        channel_id: &[u8; 32],
        event: ChannelEvent,
    ) -> Result<(), SolanaClientError> {
        let senders = self.event_senders.read().await;
        if let Some(sender) = senders.get(channel_id) {
            sender
                .send(event)
                .await
                .map_err(|_| SolanaClientError::ConnectionClosed)?;
            Ok(())
        } else {
            Err(SolanaClientError::SubscriptionError(
                "channel not subscribed".into(),
            ))
        }
    }

    /// Check if a channel is currently subscribed.
    pub async fn is_subscribed(&self, channel_id: &[u8; 32]) -> bool {
        self.event_senders.read().await.contains_key(channel_id)
    }
}

#[async_trait]
impl SolanaChannelClient for MockSolanaChannelClient {
    async fn fetch_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<Option<OnChainChannel>, SolanaClientError> {
        match &self.behavior {
            MockSolanaBehavior::RpcError => {
                Err(SolanaClientError::RpcError("mock RPC error".into()))
            }
            MockSolanaBehavior::AccountNotFound => Ok(None),
            MockSolanaBehavior::InvalidData => Err(SolanaClientError::InvalidAccountData {
                expected: OnChainChannel::LEN,
                actual: 0,
            }),
            MockSolanaBehavior::HappyPath => {
                let channels = self.channels.read().await;
                Ok(channels.get(channel_id).cloned())
            }
        }
    }

    async fn fetch_channels(
        &self,
        channel_ids: &[[u8; 32]],
    ) -> Result<Vec<([u8; 32], Option<OnChainChannel>)>, SolanaClientError> {
        match &self.behavior {
            MockSolanaBehavior::RpcError => {
                Err(SolanaClientError::RpcError("mock RPC error".into()))
            }
            MockSolanaBehavior::InvalidData => Err(SolanaClientError::InvalidAccountData {
                expected: OnChainChannel::LEN,
                actual: 0,
            }),
            MockSolanaBehavior::AccountNotFound => {
                Ok(channel_ids.iter().map(|id| (*id, None)).collect())
            }
            MockSolanaBehavior::HappyPath => {
                let channels = self.channels.read().await;
                Ok(channel_ids
                    .iter()
                    .map(|id| (*id, channels.get(id).cloned()))
                    .collect())
            }
        }
    }

    fn derive_channel_pda(&self, user: &[u8; 32], worker: &[u8; 32]) -> [u8; 32] {
        use solana_sdk::pubkey::Pubkey;

        let program_id = Pubkey::new_from_array(self.program_id);
        let seeds: &[&[u8]] = &[b"channel", user, worker];

        let (pda, _bump) = Pubkey::find_program_address(seeds, &program_id);
        pda.to_bytes()
    }

    async fn subscribe_channel(
        &self,
        channel_id: &[u8; 32],
    ) -> Result<tokio::sync::mpsc::Receiver<ChannelEvent>, SolanaClientError> {
        match &self.behavior {
            MockSolanaBehavior::RpcError => Err(SolanaClientError::SubscriptionError(
                "mock subscription error".into(),
            )),
            _ => {
                let (tx, rx) = tokio::sync::mpsc::channel(32);
                self.event_senders.write().await.insert(*channel_id, tx);
                Ok(rx)
            }
        }
    }

    async fn unsubscribe_channel(&self, channel_id: &[u8; 32]) -> Result<(), SolanaClientError> {
        self.event_senders.write().await.remove(channel_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_channel() -> OnChainChannel {
        OnChainChannel {
            user: [1u8; 32],
            worker: [2u8; 32],
            mint: [3u8; 32],
            balance: 10_000_000,
            spent: 1_000_000,
            last_nonce: 5,
            timeout: 0,
            state: OnChainChannelState::Open,
            bump: 255,
        }
    }

    #[test]
    fn test_on_chain_channel_from_bytes_valid() {
        let channel = make_test_channel();
        let bytes = channel.to_bytes();

        assert_eq!(bytes.len(), OnChainChannel::LEN);

        let parsed = OnChainChannel::from_bytes(&bytes).expect("should parse");

        assert_eq!(parsed.user, channel.user);
        assert_eq!(parsed.worker, channel.worker);
        assert_eq!(parsed.mint, channel.mint);
        assert_eq!(parsed.balance, channel.balance);
        assert_eq!(parsed.spent, channel.spent);
        assert_eq!(parsed.last_nonce, channel.last_nonce);
        assert_eq!(parsed.timeout, channel.timeout);
        assert_eq!(parsed.state, channel.state);
        assert_eq!(parsed.bump, channel.bump);
    }

    #[test]
    fn test_on_chain_channel_from_bytes_closing_state() {
        let mut channel = make_test_channel();
        channel.state = OnChainChannelState::Closing;
        channel.timeout = 1700000000;

        let bytes = channel.to_bytes();
        let parsed = OnChainChannel::from_bytes(&bytes).expect("should parse");

        assert_eq!(parsed.state, OnChainChannelState::Closing);
        assert_eq!(parsed.timeout, 1700000000);
    }

    #[test]
    fn test_on_chain_channel_from_bytes_invalid_length_too_short() {
        let bytes = vec![0u8; 100]; // Too short
        let result = OnChainChannel::from_bytes(&bytes);

        assert!(matches!(
            result,
            Err(SolanaClientError::InvalidAccountData {
                expected: 138,
                actual: 100
            })
        ));
    }

    #[test]
    fn test_on_chain_channel_from_bytes_invalid_length_too_long() {
        let bytes = vec![0u8; 200]; // Too long
        let result = OnChainChannel::from_bytes(&bytes);

        assert!(matches!(
            result,
            Err(SolanaClientError::InvalidAccountData {
                expected: 138,
                actual: 200
            })
        ));
    }

    #[test]
    fn test_on_chain_channel_from_bytes_invalid_state() {
        let channel = make_test_channel();
        let mut bytes = channel.to_bytes();

        // Set invalid state byte (position 8 + 96 + 32 = 136... wait, let me recalculate)
        // Discriminator (8) + user (32) + worker (32) + mint (32) + balance (8) + spent (8) + last_nonce (8) + timeout (8) = 136
        // State is at byte 136
        bytes[8 + 32 + 32 + 32 + 8 + 8 + 8 + 8] = 99; // Invalid state

        let result = OnChainChannel::from_bytes(&bytes);
        assert!(matches!(result, Err(SolanaClientError::ParseError(_))));
    }

    #[test]
    fn test_derive_channel_pda_deterministic() {
        let client = MockSolanaChannelClient::default().with_program_id([42u8; 32]);

        let user = [1u8; 32];
        let worker = [2u8; 32];

        let pda1 = client.derive_channel_pda(&user, &worker);
        let pda2 = client.derive_channel_pda(&user, &worker);

        assert_eq!(pda1, pda2, "PDA derivation should be deterministic");
    }

    #[test]
    fn test_derive_channel_pda_different_inputs() {
        let client = MockSolanaChannelClient::default().with_program_id([42u8; 32]);

        let user1 = [1u8; 32];
        let user2 = [2u8; 32];
        let worker = [3u8; 32];

        let pda1 = client.derive_channel_pda(&user1, &worker);
        let pda2 = client.derive_channel_pda(&user2, &worker);

        assert_ne!(pda1, pda2, "Different users should produce different PDAs");
    }

    #[tokio::test]
    async fn test_mock_inject_and_fetch_channel() {
        let client = MockSolanaChannelClient::happy_path();
        let channel = make_test_channel();
        let channel_id = [99u8; 32];

        // Initially not found
        let result = client.fetch_channel(&channel_id).await.unwrap();
        assert!(result.is_none());

        // Inject and fetch
        client.inject_channel(channel_id, channel.clone()).await;
        let result = client.fetch_channel(&channel_id).await.unwrap();
        assert!(result.is_some());

        let fetched = result.unwrap();
        assert_eq!(fetched.balance, channel.balance);
        assert_eq!(fetched.user, channel.user);
    }

    #[tokio::test]
    async fn test_mock_fetch_channels_batch() {
        let client = MockSolanaChannelClient::happy_path();

        let id1 = [1u8; 32];
        let id2 = [2u8; 32];
        let id3 = [3u8; 32];

        let channel1 = make_test_channel();
        let mut channel2 = make_test_channel();
        channel2.balance = 20_000_000;

        client.inject_channel(id1, channel1).await;
        client.inject_channel(id2, channel2).await;
        // id3 is not injected

        let results = client.fetch_channels(&[id1, id2, id3]).await.unwrap();

        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_some());
        assert!(results[1].1.is_some());
        assert!(results[2].1.is_none());

        assert_eq!(results[0].1.as_ref().unwrap().balance, 10_000_000);
        assert_eq!(results[1].1.as_ref().unwrap().balance, 20_000_000);
    }

    #[tokio::test]
    async fn test_mock_rpc_error_behavior() {
        let client = MockSolanaChannelClient::rpc_error();
        let channel_id = [1u8; 32];

        let result = client.fetch_channel(&channel_id).await;
        assert!(matches!(result, Err(SolanaClientError::RpcError(_))));

        let result = client.fetch_channels(&[channel_id]).await;
        assert!(matches!(result, Err(SolanaClientError::RpcError(_))));
    }

    #[tokio::test]
    async fn test_mock_account_not_found_behavior() {
        let client = MockSolanaChannelClient::new(MockSolanaBehavior::AccountNotFound);

        // Inject a channel - it should still return None due to behavior
        client.inject_channel([1u8; 32], make_test_channel()).await;

        let result = client.fetch_channel(&[1u8; 32]).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_subscribe_and_trigger_event() {
        let client = MockSolanaChannelClient::happy_path();
        let channel_id = [1u8; 32];

        let mut rx = client.subscribe_channel(&channel_id).await.unwrap();

        assert!(client.is_subscribed(&channel_id).await);

        // Trigger an event
        let event = ChannelEvent::TopUp {
            channel_id,
            new_balance: 20_000_000,
        };
        client.trigger_event(&channel_id, event).await.unwrap();

        // Receive the event
        let received = rx.recv().await.expect("should receive event");
        match received {
            ChannelEvent::TopUp { new_balance, .. } => {
                assert_eq!(new_balance, 20_000_000);
            }
            _ => panic!("unexpected event type"),
        }
    }

    #[tokio::test]
    async fn test_mock_unsubscribe() {
        let client = MockSolanaChannelClient::happy_path();
        let channel_id = [1u8; 32];

        let _rx = client.subscribe_channel(&channel_id).await.unwrap();
        assert!(client.is_subscribed(&channel_id).await);

        client.unsubscribe_channel(&channel_id).await.unwrap();
        assert!(!client.is_subscribed(&channel_id).await);
    }

    #[tokio::test]
    async fn test_mock_trigger_event_not_subscribed() {
        let client = MockSolanaChannelClient::happy_path();
        let channel_id = [1u8; 32];

        let event = ChannelEvent::TopUp {
            channel_id,
            new_balance: 100,
        };
        let result = client.trigger_event(&channel_id, event).await;

        assert!(matches!(
            result,
            Err(SolanaClientError::SubscriptionError(_))
        ));
    }

    #[test]
    fn test_solana_client_error_display() {
        let err = SolanaClientError::RpcError("connection refused".into());
        assert!(err.to_string().contains("connection refused"));

        let err = SolanaClientError::AccountNotFound("abc123".into());
        assert!(err.to_string().contains("abc123"));

        let err = SolanaClientError::InvalidAccountData {
            expected: 138,
            actual: 100,
        };
        assert!(err.to_string().contains("138"));
        assert!(err.to_string().contains("100"));

        let err = SolanaClientError::ParseError("bad data".into());
        assert!(err.to_string().contains("bad data"));

        let err = SolanaClientError::SubscriptionError("ws failed".into());
        assert!(err.to_string().contains("ws failed"));

        let err = SolanaClientError::ConnectionClosed;
        assert!(err.to_string().contains("connection closed"));
    }
}
