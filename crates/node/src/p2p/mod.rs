//! P2P networking module using Iroh for blobs, gossip, and direct connections.
//!
//! This module provides the [`P2PNetwork`] trait which abstracts over P2P operations,
//! enabling mock implementations for testing.

use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, PublicKey};
use iroh_blobs::Hash;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

pub mod graphene;
pub mod messages;
pub mod mock;
pub mod types;

pub use graphene::GrapheneNode;
pub use mock::{MockGrapheneNode, MockNetwork};
pub use types::{GossipSubscription, P2PConfig, TopicId};

/// Errors that can occur during P2P operations.
#[derive(Debug)]
pub enum P2PError {
    /// Failed to initialize the P2P node
    InitError(String),
    /// Blob operation failed
    BlobError(String),
    /// Gossip operation failed
    GossipError(String),
    /// Connection failed
    ConnectionError(String),
    /// The node has been shut down
    Shutdown,
    /// I/O error
    IoError(std::io::Error),
}

impl Error for P2PError {}

impl Display for P2PError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            P2PError::InitError(msg) => write!(f, "P2P initialization error: {}", msg),
            P2PError::BlobError(msg) => write!(f, "Blob error: {}", msg),
            P2PError::GossipError(msg) => write!(f, "Gossip error: {}", msg),
            P2PError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            P2PError::Shutdown => write!(f, "P2P node has been shut down"),
            P2PError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl From<std::io::Error> for P2PError {
    fn from(e: std::io::Error) -> Self {
        P2PError::IoError(e)
    }
}

impl From<anyhow::Error> for P2PError {
    fn from(e: anyhow::Error) -> Self {
        P2PError::BlobError(e.to_string())
    }
}

/// The core P2P networking trait.
///
/// Implementations provide blob storage/retrieval, gossip messaging, and direct
/// peer connections. The trait is designed to be mockable for testing.
#[async_trait]
pub trait P2PNetwork: Send + Sync {
    /// Returns this node's public key (node ID).
    fn node_id(&self) -> PublicKey;

    /// Returns this node's full address including relay information.
    async fn node_addr(&self) -> Result<EndpointAddr, P2PError>;

    // ─── Blob Operations ───────────────────────────────────────────────────────

    /// Upload data and return its content hash.
    async fn upload_blob(&self, data: &[u8]) -> Result<Hash, P2PError>;

    /// Upload a file from disk and return its content hash.
    async fn upload_blob_from_path(&self, path: &Path) -> Result<Hash, P2PError>;

    /// Download a blob by hash, optionally from a specific peer.
    ///
    /// If `from` is `None`, attempts to find the blob via any known provider.
    async fn download_blob(
        &self,
        hash: Hash,
        from: Option<EndpointAddr>,
    ) -> Result<Vec<u8>, P2PError>;

    /// Check if a blob exists locally.
    async fn has_blob(&self, hash: Hash) -> Result<bool, P2PError>;

    // ─── Gossip Operations ─────────────────────────────────────────────────────

    /// Subscribe to a gossip topic, returning a subscription handle.
    async fn subscribe(&self, topic: TopicId) -> Result<GossipSubscription, P2PError>;

    /// Broadcast a message to all peers subscribed to a topic.
    async fn broadcast(&self, topic: TopicId, message: &[u8]) -> Result<(), P2PError>;

    // ─── Direct Connections ────────────────────────────────────────────────────

    /// Open a direct QUIC connection to a peer using the specified ALPN.
    async fn connect(&self, addr: EndpointAddr, alpn: &[u8]) -> Result<Connection, P2PError>;

    // ─── Lifecycle ─────────────────────────────────────────────────────────────

    /// Gracefully shut down the P2P node.
    async fn shutdown(&self) -> Result<(), P2PError>;
}
