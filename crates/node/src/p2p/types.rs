//! Types and configuration for P2P networking.

use iroh_gossip::api::Event as GossipEvent;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

// ─── NAT Traversal Types ──────────────────────────────────────────────────────

/// The type of network path being used for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathType {
    /// Direct UDP connection (hole-punched or LAN).
    Direct,
    /// Connection via DERP relay server.
    Relay,
}

/// Information about a single network path to a peer.
#[derive(Debug, Clone)]
pub struct PathMetrics {
    /// Whether this is a direct or relayed path.
    pub path_type: PathType,
    /// Round-trip time for this path.
    pub rtt: Duration,
    /// Whether this path is currently selected for use.
    pub is_active: bool,
    /// Remote address (IP:port or relay URL).
    pub remote_addr: String,
}

/// Quality metrics for a connection.
#[derive(Debug, Clone)]
pub struct ConnectionQuality {
    /// All known paths to the peer.
    pub paths: Vec<PathMetrics>,
    /// Whether any direct path has been discovered.
    pub has_direct_path: bool,
    /// Whether a direct path is currently being used.
    pub using_direct_path: bool,
    /// Best (lowest) RTT across all paths.
    pub best_rtt: Option<Duration>,
}

impl ConnectionQuality {
    /// Create connection quality metrics from a list of path metrics.
    pub fn from_paths(paths: Vec<PathMetrics>) -> Self {
        let has_direct_path = paths.iter().any(|p| p.path_type == PathType::Direct);
        let using_direct_path = paths
            .iter()
            .any(|p| p.path_type == PathType::Direct && p.is_active);
        let best_rtt = paths.iter().map(|p| p.rtt).min();

        Self {
            paths,
            has_direct_path,
            using_direct_path,
            best_rtt,
        }
    }
}

/// Relay server configuration for NAT traversal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RelayConfig {
    /// Disable relay servers entirely (direct connections only).
    Disabled,
    /// Use n0's default production relay servers.
    #[default]
    Default,
    /// Use n0's staging relay servers.
    Staging,
    /// Use custom relay server URLs.
    Custom(Vec<String>),
}

impl From<bool> for RelayConfig {
    fn from(use_relay: bool) -> Self {
        if use_relay {
            RelayConfig::Default
        } else {
            RelayConfig::Disabled
        }
    }
}

/// Aggregated P2P metrics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct P2PMetrics {
    /// Total connections opened since node start.
    pub connections_opened: u64,
    /// Total connections closed since node start.
    pub connections_closed: u64,
    /// Number of direct paths discovered.
    pub direct_paths: u64,
    /// Number of relay paths in use.
    pub relay_paths: u64,
    /// Total hole-punch attempts made.
    pub holepunch_attempts: u64,
    /// Current number of connections using direct paths.
    pub connections_direct: u64,
}

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

    /// Build cache announcements topic.
    ///
    /// Nodes broadcast CacheAnnouncement messages on this topic to
    /// advertise availability of cached build artifacts.
    pub fn cache_v1() -> Self {
        Self::from_name("graphene-cache-v1")
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

    /// Relay server configuration for NAT traversal.
    pub relay_config: RelayConfig,

    /// Bootstrap peers to connect to on startup.
    pub bootstrap_peers: Vec<iroh::EndpointAddr>,

    /// Port to bind to (0 for random available port).
    pub bind_port: u16,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from(".graphene"),
            relay_config: RelayConfig::Default,
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

    /// Set whether to use relay servers (backward compatibility).
    pub fn with_relay(mut self, use_relay: bool) -> Self {
        self.relay_config = use_relay.into();
        self
    }

    /// Set the relay configuration.
    pub fn with_relay_config(mut self, relay_config: RelayConfig) -> Self {
        self.relay_config = relay_config;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_quality_from_paths_empty() {
        let quality = ConnectionQuality::from_paths(vec![]);
        assert!(!quality.has_direct_path);
        assert!(!quality.using_direct_path);
        assert!(quality.best_rtt.is_none());
        assert!(quality.paths.is_empty());
    }

    #[test]
    fn test_connection_quality_from_paths_direct_only() {
        let paths = vec![PathMetrics {
            path_type: PathType::Direct,
            rtt: Duration::from_millis(25),
            is_active: true,
            remote_addr: "192.168.1.100:12345".to_string(),
        }];

        let quality = ConnectionQuality::from_paths(paths);
        assert!(quality.has_direct_path);
        assert!(quality.using_direct_path);
        assert_eq!(quality.best_rtt, Some(Duration::from_millis(25)));
        assert_eq!(quality.paths.len(), 1);
    }

    #[test]
    fn test_connection_quality_from_paths_relay_only() {
        let paths = vec![PathMetrics {
            path_type: PathType::Relay,
            rtt: Duration::from_millis(100),
            is_active: true,
            remote_addr: "relay.example.com".to_string(),
        }];

        let quality = ConnectionQuality::from_paths(paths);
        assert!(!quality.has_direct_path);
        assert!(!quality.using_direct_path);
        assert_eq!(quality.best_rtt, Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_connection_quality_from_paths_mixed() {
        let paths = vec![
            PathMetrics {
                path_type: PathType::Direct,
                rtt: Duration::from_millis(25),
                is_active: false,
                remote_addr: "192.168.1.100:12345".to_string(),
            },
            PathMetrics {
                path_type: PathType::Relay,
                rtt: Duration::from_millis(100),
                is_active: true,
                remote_addr: "relay.example.com".to_string(),
            },
        ];

        let quality = ConnectionQuality::from_paths(paths);
        assert!(quality.has_direct_path);
        assert!(!quality.using_direct_path); // Relay is active, not direct
        assert_eq!(quality.best_rtt, Some(Duration::from_millis(25)));
        assert_eq!(quality.paths.len(), 2);
    }

    #[test]
    fn test_connection_quality_using_direct_when_active() {
        let paths = vec![
            PathMetrics {
                path_type: PathType::Direct,
                rtt: Duration::from_millis(25),
                is_active: true,
                remote_addr: "192.168.1.100:12345".to_string(),
            },
            PathMetrics {
                path_type: PathType::Relay,
                rtt: Duration::from_millis(100),
                is_active: false,
                remote_addr: "relay.example.com".to_string(),
            },
        ];

        let quality = ConnectionQuality::from_paths(paths);
        assert!(quality.has_direct_path);
        assert!(quality.using_direct_path);
    }

    #[test]
    fn test_relay_config_from_bool() {
        assert_eq!(RelayConfig::from(true), RelayConfig::Default);
        assert_eq!(RelayConfig::from(false), RelayConfig::Disabled);
    }

    #[test]
    fn test_relay_config_default() {
        assert_eq!(RelayConfig::default(), RelayConfig::Default);
    }

    #[test]
    fn test_path_type_equality() {
        assert_eq!(PathType::Direct, PathType::Direct);
        assert_eq!(PathType::Relay, PathType::Relay);
        assert_ne!(PathType::Direct, PathType::Relay);
    }

    #[test]
    fn test_p2p_config_with_relay_config() {
        let config = P2PConfig::new("/tmp/test").with_relay_config(RelayConfig::Staging);
        assert_eq!(config.relay_config, RelayConfig::Staging);

        let config2 = P2PConfig::new("/tmp/test").with_relay_config(RelayConfig::Custom(vec![
            "https://relay.example.com".to_string(),
        ]));
        match config2.relay_config {
            RelayConfig::Custom(urls) => {
                assert_eq!(urls.len(), 1);
                assert_eq!(urls[0], "https://relay.example.com");
            }
            _ => panic!("Expected Custom relay config"),
        }
    }

    #[test]
    fn test_p2p_config_with_relay_backward_compat() {
        let config = P2PConfig::new("/tmp/test").with_relay(true);
        assert_eq!(config.relay_config, RelayConfig::Default);

        let config2 = P2PConfig::new("/tmp/test").with_relay(false);
        assert_eq!(config2.relay_config, RelayConfig::Disabled);
    }
}
