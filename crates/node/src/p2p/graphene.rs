//! Real Iroh-based P2P implementation for the Graphene network.
//!
//! [`GrapheneNode`] provides blob storage, gossip messaging, and direct connections
//! using Iroh's QUIC-based networking stack.

use super::{GossipSubscription, P2PConfig, P2PError, P2PNetwork, TopicId};
use async_trait::async_trait;
use iroh::endpoint::{Connection, Endpoint};
use iroh::{EndpointAddr, PublicKey, SecretKey};
use iroh_blobs::{BlobsProtocol, Hash};
use iroh_gossip::net::Gossip;
use rand::RngCore;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// ALPN protocol identifier for Graphene job requests.
pub const GRAPHENE_JOB_ALPN: &[u8] = b"graphene/job/1";

/// Real P2P node implementation using Iroh.
pub struct GrapheneNode {
    /// The QUIC endpoint for all connections.
    endpoint: Endpoint,

    /// Blob storage and transfer protocol.
    blobs: BlobsProtocol,

    /// Gossip protocol instance.
    gossip: Gossip,

    /// Node's secret key.
    secret_key: SecretKey,

    /// Storage path for persistent data.
    #[allow(dead_code)]
    storage_path: std::path::PathBuf,

    /// Active gossip subscriptions.
    subscriptions: Arc<RwLock<Vec<TopicId>>>,

    /// Whether the node is shutting down.
    shutting_down: Arc<RwLock<bool>>,
}

impl GrapheneNode {
    /// Create a new Graphene P2P node with the given configuration.
    pub async fn new(config: P2PConfig) -> Result<Self, P2PError> {
        info!("Initializing Graphene P2P node...");

        // Ensure storage directory exists
        std::fs::create_dir_all(&config.storage_path)?;

        // Load or generate identity
        let secret_key = Self::load_or_generate_identity(&config.storage_path)?;
        let node_id = secret_key.public();
        info!("Node ID: {}", node_id);

        // Build the QUIC endpoint
        let mut endpoint_builder = Endpoint::builder().secret_key(secret_key.clone());

        if !config.use_relay {
            endpoint_builder = endpoint_builder.relay_mode(iroh::RelayMode::Disabled);
        }

        // Add ALPNs for all protocols we support
        endpoint_builder = endpoint_builder.alpns(vec![
            iroh_blobs::protocol::ALPN.to_vec(),
            iroh_gossip::net::GOSSIP_ALPN.to_vec(),
            GRAPHENE_JOB_ALPN.to_vec(),
        ]);

        let endpoint = endpoint_builder
            .bind()
            .await
            .map_err(|e| P2PError::InitError(format!("Failed to bind endpoint: {}", e)))?;

        info!("Endpoint bound");

        // Initialize blob store
        let blob_store_path = config.storage_path.join("blobs");
        std::fs::create_dir_all(&blob_store_path)?;

        let blob_store = iroh_blobs::store::fs::FsStore::load(&blob_store_path)
            .await
            .map_err(|e| P2PError::InitError(format!("Failed to create blob store: {}", e)))?;

        // Initialize blobs protocol
        let blobs = BlobsProtocol::new(&blob_store, None);

        // Initialize gossip (spawn is synchronous)
        let gossip = Gossip::builder().spawn(endpoint.clone());

        info!("Graphene P2P node initialized successfully");

        Ok(Self {
            endpoint,
            blobs,
            gossip,
            secret_key,
            storage_path: config.storage_path,
            subscriptions: Arc::new(RwLock::new(Vec::new())),
            shutting_down: Arc::new(RwLock::new(false)),
        })
    }

    /// Load identity from disk or generate a new one.
    fn load_or_generate_identity(storage_path: &Path) -> Result<SecretKey, P2PError> {
        let identity_path = storage_path.join("identity.key");

        if identity_path.exists() {
            let key_bytes = std::fs::read(&identity_path)?;
            if key_bytes.len() == 32 {
                let key_array: [u8; 32] = key_bytes
                    .try_into()
                    .map_err(|_| P2PError::InitError("Invalid identity key length".into()))?;
                let secret_key = SecretKey::from_bytes(&key_array);
                info!("Loaded existing identity from {:?}", identity_path);
                return Ok(secret_key);
            }
        }

        // Generate new identity using random bytes
        let mut key_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let secret_key = SecretKey::from_bytes(&key_bytes);

        // Persist it
        std::fs::write(&identity_path, secret_key.to_bytes())?;
        info!("Generated new identity, saved to {:?}", identity_path);

        Ok(secret_key)
    }

    /// Get the underlying endpoint for advanced usage.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get the blobs protocol instance.
    pub fn blobs(&self) -> &BlobsProtocol {
        &self.blobs
    }

    /// Get the gossip instance.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    /// Accept incoming connections in a loop.
    ///
    /// This should be spawned as a background task. Handles job requests
    /// via the provided handler. Blob and gossip connections are handled
    /// internally by their respective protocols.
    pub async fn accept_loop<F, Fut>(self: Arc<Self>, handler: Arc<F>)
    where
        F: Fn(Connection, Arc<Self>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), P2PError>> + Send,
    {
        loop {
            if *self.shutting_down.read().await {
                break;
            }

            match self.endpoint.accept().await {
                Some(incoming) => {
                    let self_clone = self.clone();
                    let handler_clone = handler.clone();

                    // Accept the connection first
                    let conn = match incoming.await {
                        Ok(conn) => conn,
                        Err(e) => {
                            warn!("Failed to accept connection: {}", e);
                            continue;
                        }
                    };

                    let alpn = conn.alpn();

                    // Route based on ALPN - only handle job requests here
                    // Blob and gossip protocols are handled via the Router pattern
                    if alpn == GRAPHENE_JOB_ALPN {
                        debug!("Incoming job request");
                        tokio::spawn(async move {
                            if let Err(e) = handler_clone(conn, self_clone).await {
                                warn!("Job handler error: {}", e);
                            }
                        });
                    } else if alpn == iroh_gossip::net::GOSSIP_ALPN {
                        debug!("Incoming gossip connection");
                        let gossip = self.gossip.clone();
                        tokio::spawn(async move {
                            if let Err(e) = gossip.handle_connection(conn).await {
                                warn!("Gossip connection error: {}", e);
                            }
                        });
                    } else {
                        debug!("Connection with ALPN: {:?}", String::from_utf8_lossy(alpn));
                    }
                }
                None => break,
            }
        }
    }
}

#[async_trait]
impl P2PNetwork for GrapheneNode {
    fn node_id(&self) -> PublicKey {
        self.secret_key.public()
    }

    async fn node_addr(&self) -> Result<EndpointAddr, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        Ok(self.endpoint.addr())
    }

    async fn upload_blob(&self, data: &[u8]) -> Result<Hash, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        let tag = self
            .blobs
            .add_bytes(data.to_vec())
            .await
            .map_err(|e| P2PError::BlobError(format!("Failed to add blob: {}", e)))?;

        Ok(tag.hash)
    }

    async fn upload_blob_from_path(&self, path: &Path) -> Result<Hash, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        let abs_path = path.canonicalize().map_err(P2PError::IoError)?;

        let import = self
            .blobs
            .add_path(abs_path)
            .await
            .map_err(|e| P2PError::BlobError(format!("Failed to import file: {}", e)))?;

        Ok(import.hash)
    }

    async fn download_blob(
        &self,
        hash: Hash,
        _from: Option<EndpointAddr>,
    ) -> Result<Vec<u8>, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        // Check if we have it locally
        let has_locally = self
            .blobs
            .has(hash)
            .await
            .map_err(|e| P2PError::BlobError(format!("Failed to check blob: {}", e)))?;

        if has_locally {
            let data = self
                .blobs
                .get_bytes(hash)
                .await
                .map_err(|e| P2PError::BlobError(format!("Failed to read blob: {}", e)))?;

            return Ok(data.to_vec());
        }

        // TODO: Implement remote download using the `from` address
        // For now, return error if not found locally
        Err(P2PError::BlobError(format!(
            "Blob {} not found locally",
            hash
        )))
    }

    async fn has_blob(&self, hash: Hash) -> Result<bool, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        self.blobs
            .has(hash)
            .await
            .map_err(|e| P2PError::BlobError(format!("Failed to check blob: {}", e)))
    }

    async fn subscribe(&self, topic: TopicId) -> Result<GossipSubscription, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        // Convert to iroh-gossip topic
        let iroh_topic: iroh_gossip::proto::TopicId = topic.into();

        // Subscribe to the topic
        let gossip_topic = self
            .gossip
            .subscribe(iroh_topic, vec![])
            .await
            .map_err(|e| P2PError::GossipError(format!("Failed to subscribe: {}", e)))?;

        // Track subscription
        self.subscriptions.write().await.push(topic);

        // Create channels for the subscription interface
        let (event_tx, event_rx) = mpsc::channel::<iroh_gossip::api::Event>(100);
        let (broadcast_tx, mut broadcast_rx) = mpsc::channel::<Vec<u8>>(100);

        // Split the gossip topic into sender and receiver
        let (sender, mut receiver) = gossip_topic.split();

        // Forward incoming gossip events
        tokio::spawn(async move {
            use futures_lite::StreamExt;
            while let Some(event) = receiver.next().await {
                match event {
                    Ok(ev) => {
                        if event_tx.send(ev).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Gossip receive error: {}", e);
                        break;
                    }
                }
            }
        });

        // Handle outgoing broadcasts
        tokio::spawn(async move {
            while let Some(msg) = broadcast_rx.recv().await {
                if let Err(e) = sender.broadcast(msg.into()).await {
                    warn!("Failed to broadcast: {}", e);
                    break;
                }
            }
        });

        Ok(GossipSubscription::new(topic, event_rx, broadcast_tx))
    }

    async fn broadcast(&self, topic: TopicId, message: &[u8]) -> Result<(), P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        let iroh_topic: iroh_gossip::proto::TopicId = topic.into();

        // We need an active subscription to broadcast
        let gossip_topic = self
            .gossip
            .subscribe(iroh_topic, vec![])
            .await
            .map_err(|e| {
                P2PError::GossipError(format!("Failed to subscribe for broadcast: {}", e))
            })?;

        let (sender, _receiver) = gossip_topic.split();

        sender
            .broadcast(message.to_vec().into())
            .await
            .map_err(|e| P2PError::GossipError(format!("Failed to broadcast: {}", e)))?;

        Ok(())
    }

    async fn connect(&self, addr: EndpointAddr, alpn: &[u8]) -> Result<Connection, P2PError> {
        if *self.shutting_down.read().await {
            return Err(P2PError::Shutdown);
        }

        self.endpoint
            .connect(addr, alpn)
            .await
            .map_err(|e| P2PError::ConnectionError(format!("Failed to connect: {}", e)))
    }

    async fn shutdown(&self) -> Result<(), P2PError> {
        info!("Shutting down Graphene P2P node...");
        *self.shutting_down.write().await = true;

        // Shutdown gossip
        let _ = self.gossip.shutdown().await;

        // Shutdown blobs protocol (releases database locks)
        let _ = self.blobs.shutdown().await;

        // Close the endpoint
        self.endpoint.close().await;

        info!("Graphene P2P node shut down");
        Ok(())
    }
}
