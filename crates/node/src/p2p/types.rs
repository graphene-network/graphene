//! Types and configuration for P2P networking.

use iroh_gossip::api::Event as GossipEvent;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Topic identifier for gossip subscriptions.
///
/// Topics are 32-byte identifiers derived from human-readable names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TopicId(pub [u8; 32]);

impl TopicId {
    /// Create a topic ID from a human-readable name.
    ///
    /// Uses BLAKE3 to derive a 32-byte topic ID from the name.
    pub fn from_name(name: &str) -> Self {
        let hash = blake3::hash(name.as_bytes());
        TopicId(*hash.as_bytes())
    }

    /// Worker discovery topic for the Graphene compute network.
    pub fn compute_v1() -> Self {
        Self::from_name("graphene-compute-v1")
    }

    /// Double-spend prevention topic for payment tickets.
    pub fn tickets_v1() -> Self {
        Self::from_name("graphene-tickets-v1")
    }

    /// Get the raw bytes of this topic ID.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<TopicId> for iroh_gossip::proto::TopicId {
    fn from(topic: TopicId) -> Self {
        iroh_gossip::proto::TopicId::from_bytes(topic.0)
    }
}

impl From<iroh_gossip::proto::TopicId> for TopicId {
    fn from(topic: iroh_gossip::proto::TopicId) -> Self {
        TopicId(*topic.as_bytes())
    }
}

/// Configuration for initializing a P2P node.
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// Path for persistent storage (identity key, blob store).
    pub storage_path: PathBuf,

    /// Whether to use the default Iroh relay servers.
    /// If false, the node operates in direct-connection-only mode.
    pub use_relay: bool,

    /// Bootstrap peers to connect to on startup.
    pub bootstrap_peers: Vec<iroh::EndpointAddr>,

    /// Port to bind to (0 for random available port).
    pub bind_port: u16,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from(".graphene"),
            use_relay: true,
            bootstrap_peers: Vec::new(),
            bind_port: 0,
        }
    }
}

impl P2PConfig {
    /// Create a new config with the specified storage path.
    pub fn new(storage_path: impl Into<PathBuf>) -> Self {
        Self {
            storage_path: storage_path.into(),
            ..Default::default()
        }
    }

    /// Set whether to use relay servers.
    pub fn with_relay(mut self, use_relay: bool) -> Self {
        self.use_relay = use_relay;
        self
    }

    /// Add bootstrap peers.
    pub fn with_bootstrap_peers(mut self, peers: Vec<iroh::EndpointAddr>) -> Self {
        self.bootstrap_peers = peers;
        self
    }

    /// Set the bind port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_port = port;
        self
    }
}

/// A subscription to a gossip topic.
///
/// Provides a receiver for incoming gossip events and a sender for broadcasting.
pub struct GossipSubscription {
    /// Receiver for incoming gossip events.
    pub receiver: mpsc::Receiver<GossipEvent>,

    /// Sender for broadcasting messages to the topic.
    pub sender: mpsc::Sender<Vec<u8>>,

    /// The topic this subscription is for.
    pub topic: TopicId,
}

impl GossipSubscription {
    /// Create a new gossip subscription.
    pub fn new(
        topic: TopicId,
        receiver: mpsc::Receiver<GossipEvent>,
        sender: mpsc::Sender<Vec<u8>>,
    ) -> Self {
        Self {
            receiver,
            sender,
            topic,
        }
    }

    /// Receive the next gossip event.
    pub async fn recv(&mut self) -> Option<GossipEvent> {
        self.receiver.recv().await
    }

    /// Broadcast a message to the topic.
    pub async fn broadcast(&self, message: Vec<u8>) -> Result<(), mpsc::error::SendError<Vec<u8>>> {
        self.sender.send(message).await
    }
}
