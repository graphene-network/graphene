//! Mock P2P implementation for testing.
//!
//! Provides [`MockGrapheneNode`] which implements [`P2PNetwork`] with configurable
//! behaviors for testing different scenarios.

use super::{GossipSubscription, P2PError, P2PNetwork, TopicId};
use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, PublicKey, SecretKey};
use iroh_blobs::Hash;
use iroh_gossip::api::Event as GossipEvent;
use rand::RngCore;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Configurable behaviors for the mock P2P node.
#[derive(Debug, Clone, Default)]
pub enum MockBehavior {
    /// Normal operation - all operations succeed.
    #[default]
    HappyPath,

    /// Blob downloads fail.
    BlobDownloadFailure,

    /// Gossip subscription fails.
    GossipFailure,

    /// Connection attempts fail.
    ConnectionFailure,

    /// Node is shut down.
    Shutdown,
}

/// Spy state for tracking operations in tests.
#[derive(Debug, Default)]
pub struct MockSpyState {
    /// Blobs that were uploaded.
    pub uploaded_blobs: Vec<(Hash, Vec<u8>)>,

    /// Blob download attempts.
    pub download_attempts: Vec<(Hash, Option<EndpointAddr>)>,

    /// Topics that were subscribed to.
    pub subscribed_topics: Vec<TopicId>,

    /// Messages that were broadcast.
    pub broadcast_messages: Vec<(TopicId, Vec<u8>)>,

    /// Connection attempts.
    pub connection_attempts: Vec<(EndpointAddr, Vec<u8>)>,

    /// Whether shutdown was called.
    pub shutdown_called: bool,
}

/// A shared network for connecting multiple mock nodes in tests.
#[derive(Debug, Default, Clone)]
pub struct MockNetwork {
    /// Blobs shared across all nodes.
    blobs: Arc<RwLock<HashMap<Hash, Vec<u8>>>>,

    /// Gossip messages by topic.
    gossip: Arc<RwLock<HashMap<TopicId, Vec<Vec<u8>>>>>,
}

impl MockNetwork {
    /// Create a new mock network.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a blob from the shared network.
    pub fn get_blob(&self, hash: &Hash) -> Option<Vec<u8>> {
        self.blobs.read().unwrap().get(hash).cloned()
    }

    /// Put a blob into the shared network.
    pub fn put_blob(&self, hash: Hash, data: Vec<u8>) {
        self.blobs.write().unwrap().insert(hash, data);
    }

    /// Get all gossip messages for a topic.
    pub fn get_gossip(&self, topic: &TopicId) -> Vec<Vec<u8>> {
        self.gossip
            .read()
            .unwrap()
            .get(topic)
            .cloned()
            .unwrap_or_default()
    }

    /// Broadcast a gossip message.
    pub fn broadcast_gossip(&self, topic: TopicId, message: Vec<u8>) {
        self.gossip
            .write()
            .unwrap()
            .entry(topic)
            .or_default()
            .push(message);
    }
}

/// Mock implementation of [`P2PNetwork`] for testing.
pub struct MockGrapheneNode {
    /// The node's secret key.
    secret_key: SecretKey,

    /// Current behavior mode.
    behavior: Arc<RwLock<MockBehavior>>,

    /// Spy state for assertions.
    spy: Arc<RwLock<MockSpyState>>,

    /// Local blob storage.
    local_blobs: Arc<RwLock<HashMap<Hash, Vec<u8>>>>,

    /// Optional shared network for multi-node tests.
    network: Option<MockNetwork>,

    /// Active gossip subscriptions (topic -> sender for injecting events).
    gossip_injectors: Arc<RwLock<HashMap<TopicId, mpsc::Sender<GossipEvent>>>>,
}

impl MockGrapheneNode {
    /// Create a new mock node with the default happy path behavior.
    pub fn new() -> Self {
        Self::with_behavior(MockBehavior::HappyPath)
    }

    /// Create a new mock node with a specific behavior.
    pub fn with_behavior(behavior: MockBehavior) -> Self {
        // Generate random bytes for the secret key
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);

        Self {
            secret_key: SecretKey::from_bytes(&key_bytes),
            behavior: Arc::new(RwLock::new(behavior)),
            spy: Arc::new(RwLock::new(MockSpyState::default())),
            local_blobs: Arc::new(RwLock::new(HashMap::new())),
            network: None,
            gossip_injectors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a mock node connected to a shared network.
    pub fn with_network(network: MockNetwork) -> Self {
        let mut node = Self::new();
        node.network = Some(network);
        node
    }

    /// Get access to the spy state for assertions.
    pub fn spy(&self) -> impl std::ops::Deref<Target = MockSpyState> + '_ {
        self.spy.read().unwrap()
    }

    /// Set the mock behavior.
    pub fn set_behavior(&self, behavior: MockBehavior) {
        *self.behavior.write().unwrap() = behavior;
    }

    /// Pre-populate a blob in local storage.
    pub fn inject_blob(&self, hash: Hash, data: Vec<u8>) {
        self.local_blobs.write().unwrap().insert(hash, data.clone());
        if let Some(ref network) = self.network {
            network.put_blob(hash, data);
        }
    }

    /// Inject a gossip event into an active subscription.
    pub async fn inject_gossip_event(
        &self,
        topic: TopicId,
        event: GossipEvent,
    ) -> bool {
        if let Some(sender) = self.gossip_injectors.read().unwrap().get(&topic) {
            sender.send(event).await.is_ok()
        } else {
            false
        }
    }

    fn check_behavior(&self) -> Result<(), P2PError> {
        match *self.behavior.read().unwrap() {
            MockBehavior::Shutdown => Err(P2PError::Shutdown),
            _ => Ok(()),
        }
    }
}

impl Default for MockGrapheneNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl P2PNetwork for MockGrapheneNode {
    fn node_id(&self) -> PublicKey {
        self.secret_key.public()
    }

    async fn node_addr(&self) -> Result<EndpointAddr, P2PError> {
        self.check_behavior()?;
        Ok(EndpointAddr::new(self.secret_key.public()))
    }

    async fn upload_blob(&self, data: &[u8]) -> Result<Hash, P2PError> {
        self.check_behavior()?;

        let hash = Hash::new(data);
        self.local_blobs
            .write()
            .unwrap()
            .insert(hash, data.to_vec());

        // Record in spy state
        self.spy
            .write()
            .unwrap()
            .uploaded_blobs
            .push((hash, data.to_vec()));

        // Share with network if connected
        if let Some(ref network) = self.network {
            network.put_blob(hash, data.to_vec());
        }

        Ok(hash)
    }

    async fn upload_blob_from_path(&self, path: &Path) -> Result<Hash, P2PError> {
        self.check_behavior()?;

        let data = std::fs::read(path)?;
        self.upload_blob(&data).await
    }

    async fn download_blob(
        &self,
        hash: Hash,
        from: Option<EndpointAddr>,
    ) -> Result<Vec<u8>, P2PError> {
        self.check_behavior()?;

        // Record attempt
        self.spy
            .write()
            .unwrap()
            .download_attempts
            .push((hash, from));

        if matches!(
            *self.behavior.read().unwrap(),
            MockBehavior::BlobDownloadFailure
        ) {
            return Err(P2PError::BlobError("Mock download failure".into()));
        }

        // Check local storage first
        if let Some(data) = self.local_blobs.read().unwrap().get(&hash) {
            return Ok(data.clone());
        }

        // Check shared network
        if let Some(ref network) = self.network {
            if let Some(data) = network.get_blob(&hash) {
                return Ok(data);
            }
        }

        Err(P2PError::BlobError(format!("Blob {} not found", hash)))
    }

    async fn has_blob(&self, hash: Hash) -> Result<bool, P2PError> {
        self.check_behavior()?;

        if self.local_blobs.read().unwrap().contains_key(&hash) {
            return Ok(true);
        }

        if let Some(ref network) = self.network {
            if network.get_blob(&hash).is_some() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn subscribe(&self, topic: TopicId) -> Result<GossipSubscription, P2PError> {
        self.check_behavior()?;

        if matches!(*self.behavior.read().unwrap(), MockBehavior::GossipFailure) {
            return Err(P2PError::GossipError("Mock gossip failure".into()));
        }

        // Record subscription
        self.spy.write().unwrap().subscribed_topics.push(topic);

        // Create channels for the subscription
        let (event_tx, event_rx) = mpsc::channel(100);
        let (broadcast_tx, mut broadcast_rx) = mpsc::channel::<Vec<u8>>(100);

        // Store the injector for tests
        self.gossip_injectors
            .write()
            .unwrap()
            .insert(topic, event_tx);

        // Handle broadcasts by recording them and optionally sharing with network
        let spy = self.spy.clone();
        let network = self.network.clone();
        tokio::spawn(async move {
            while let Some(msg) = broadcast_rx.recv().await {
                spy.write()
                    .unwrap()
                    .broadcast_messages
                    .push((topic, msg.clone()));
                if let Some(ref net) = network {
                    net.broadcast_gossip(topic, msg);
                }
            }
        });

        Ok(GossipSubscription::new(topic, event_rx, broadcast_tx))
    }

    async fn broadcast(&self, topic: TopicId, message: &[u8]) -> Result<(), P2PError> {
        self.check_behavior()?;

        if matches!(*self.behavior.read().unwrap(), MockBehavior::GossipFailure) {
            return Err(P2PError::GossipError("Mock gossip failure".into()));
        }

        // Record the broadcast
        self.spy
            .write()
            .unwrap()
            .broadcast_messages
            .push((topic, message.to_vec()));

        // Share with network
        if let Some(ref network) = self.network {
            network.broadcast_gossip(topic, message.to_vec());
        }

        Ok(())
    }

    async fn connect(&self, addr: EndpointAddr, alpn: &[u8]) -> Result<Connection, P2PError> {
        self.check_behavior()?;

        // Record attempt
        self.spy
            .write()
            .unwrap()
            .connection_attempts
            .push((addr.clone(), alpn.to_vec()));

        if matches!(
            *self.behavior.read().unwrap(),
            MockBehavior::ConnectionFailure
        ) {
            return Err(P2PError::ConnectionError("Mock connection failure".into()));
        }

        // In mock mode, we can't actually create a real Connection
        // Tests that need actual connections should use integration tests
        Err(P2PError::ConnectionError(
            "Mock node cannot create real connections - use integration tests".into(),
        ))
    }

    async fn shutdown(&self) -> Result<(), P2PError> {
        self.spy.write().unwrap().shutdown_called = true;
        *self.behavior.write().unwrap() = MockBehavior::Shutdown;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_blob_upload_download() {
        let node = MockGrapheneNode::new();

        let data = b"hello world";
        let hash = node.upload_blob(data).await.unwrap();

        let downloaded = node.download_blob(hash, None).await.unwrap();
        assert_eq!(downloaded, data);

        assert!(node.has_blob(hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_network_sharing() {
        let network = MockNetwork::new();
        let node1 = MockGrapheneNode::with_network(network.clone());
        let node2 = MockGrapheneNode::with_network(network);

        // Upload from node1
        let data = b"shared data";
        let hash = node1.upload_blob(data).await.unwrap();

        // Download from node2
        let downloaded = node2.download_blob(hash, None).await.unwrap();
        assert_eq!(downloaded, data);
    }

    #[tokio::test]
    async fn test_mock_behavior_failure() {
        let node = MockGrapheneNode::with_behavior(MockBehavior::BlobDownloadFailure);

        let data = b"test";
        let hash = node.upload_blob(data).await.unwrap();

        let result = node.download_blob(hash, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_spy_state() {
        let node = MockGrapheneNode::new();

        node.upload_blob(b"data1").await.unwrap();
        node.upload_blob(b"data2").await.unwrap();

        assert_eq!(node.spy().uploaded_blobs.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_shutdown() {
        let node = MockGrapheneNode::new();

        node.shutdown().await.unwrap();

        assert!(node.spy().shutdown_called);
        assert!(node.upload_blob(b"test").await.is_err());
    }
}
