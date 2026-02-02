# P2P Module

Peer-to-peer networking for the Graphene compute network using [Iroh](https://iroh.computer) 0.96.

## Overview

This module provides content-addressed blob storage, gossip-based pub/sub messaging, and direct QUIC connections between nodes. All functionality is abstracted behind the `P2PNetwork` trait to enable mock implementations for testing.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        P2PNetwork Trait                         │
├─────────────────────────────────────────────────────────────────┤
│  Blobs          │  Gossip           │  Connections              │
│  - upload       │  - subscribe      │  - connect (QUIC)         │
│  - download     │  - broadcast      │  - accept_loop            │
│  - has_blob     │                   │                           │
└─────────────────────────────────────────────────────────────────┘
         │                   │                      │
         ▼                   ▼                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                     GrapheneNode (Real)                         │
│  - iroh Endpoint (QUIC)                                         │
│  - iroh-blobs (content-addressed storage)                       │
│  - iroh-gossip (pub/sub messaging)                              │
│  - Persistent identity                                          │
└─────────────────────────────────────────────────────────────────┘
```

## Files

| File | Description |
|------|-------------|
| `mod.rs` | `P2PNetwork` trait definition and `P2PError` types |
| `graphene.rs` | Production implementation using Iroh |
| `mock.rs` | Test double with configurable behaviors and spy state |
| `types.rs` | `P2PConfig`, `TopicId`, and `GossipSubscription` |
| `messages.rs` | Gossip message types for worker discovery and payments |

## P2PNetwork Trait

The core abstraction for P2P operations:

```rust
#[async_trait]
pub trait P2PNetwork: Send + Sync {
    // Identity
    fn node_id(&self) -> PublicKey;
    async fn node_addr(&self) -> Result<EndpointAddr, P2PError>;

    // Blob operations
    async fn upload_blob(&self, data: &[u8]) -> Result<Hash, P2PError>;
    async fn upload_blob_from_path(&self, path: &Path) -> Result<Hash, P2PError>;
    async fn download_blob(&self, hash: Hash, from: Option<EndpointAddr>) -> Result<Vec<u8>, P2PError>;
    async fn has_blob(&self, hash: Hash) -> Result<bool, P2PError>;

    // Gossip operations
    async fn subscribe(&self, topic: TopicId) -> Result<GossipSubscription, P2PError>;
    async fn broadcast(&self, topic: TopicId, message: &[u8]) -> Result<(), P2PError>;

    // Direct connections
    async fn connect(&self, addr: EndpointAddr, alpn: &[u8]) -> Result<Connection, P2PError>;

    // Lifecycle
    async fn shutdown(&self) -> Result<(), P2PError>;
}
```

## Configuration

```rust
let config = P2PConfig::new("/path/to/storage")
    .with_relay(true)           // Use Iroh relay servers (default: true)
    .with_port(4433)            // Bind port (default: 0 = random)
    .with_bootstrap_peers(vec![peer_addr]);

let node = GrapheneNode::new(config).await?;
```

The node persists its identity key at `{storage_path}/identity.key` and stores blobs in `{storage_path}/blobs/`.

## Topics

Topics are 32-byte identifiers derived from human-readable names using BLAKE3:

```rust
let topic = TopicId::from_name("my-custom-topic");

// Built-in topics:
let compute = TopicId::compute_v1();  // "graphene-compute-v1" - worker discovery
let tickets = TopicId::tickets_v1();  // "graphene-tickets-v1" - payment double-spend prevention
```

## Message Types

### Compute Messages (`graphene-compute-v1`)

- `WorkerAnnouncement` - Worker advertising availability with capabilities and pricing
- `WorkerHeartbeat` - Periodic liveness signal with load information
- `DiscoveryQuery` - Client searching for workers matching criteria
- `DiscoveryResponse` - Worker responding to a discovery query

### Ticket Messages (`graphene-tickets-v1`)

- `TicketAccepted` - Worker accepted a payment ticket (claims it)
- `TicketRejected` - Ticket rejected (already spent)

## Testing

### MockGrapheneNode

Provides a test double with configurable failure modes:

```rust
use graphene_node::p2p::{MockGrapheneNode, MockBehavior, MockNetwork};

// Happy path
let node = MockGrapheneNode::new();

// Simulate failures
let node = MockGrapheneNode::with_behavior(MockBehavior::BlobDownloadFailure);
let node = MockGrapheneNode::with_behavior(MockBehavior::GossipFailure);
let node = MockGrapheneNode::with_behavior(MockBehavior::ConnectionFailure);
```

### Spy State

Inspect what operations were performed:

```rust
let node = MockGrapheneNode::new();
node.upload_blob(b"data").await?;
node.broadcast(topic, b"message").await?;

assert_eq!(node.spy().uploaded_blobs.len(), 1);
assert_eq!(node.spy().broadcast_messages.len(), 1);
```

### MockNetwork

Connect multiple mock nodes for multi-node tests:

```rust
let network = MockNetwork::new();
let node1 = MockGrapheneNode::with_network(network.clone());
let node2 = MockGrapheneNode::with_network(network);

// Blobs and gossip messages are shared across the network
let hash = node1.upload_blob(b"shared").await?;
let data = node2.download_blob(hash, None).await?;
```

### Injecting Events

Simulate incoming gossip events:

```rust
use iroh_gossip::api::Event;

let node = MockGrapheneNode::new();
let mut sub = node.subscribe(topic).await?;

// Inject a received message from another test
node.inject_gossip_event(topic, Event::Received { ... }).await;
```

## ALPN Protocols

The node registers these ALPN identifiers:

| ALPN | Purpose |
|------|---------|
| `iroh-blobs/1` | Blob transfer protocol |
| `iroh-gossip/0` | Gossip messaging |
| `graphene/job/1` | Job request handling |

## Accept Loop

For production nodes that accept incoming connections:

```rust
let node = Arc::new(GrapheneNode::new(config).await?);

// Handler for job requests
let handler = Arc::new(|conn: Connection, node: Arc<GrapheneNode>| async move {
    // Handle the job request
    Ok(())
});

// Spawn the accept loop
tokio::spawn(node.clone().accept_loop(handler));
```

Blob and gossip connections are handled internally by their respective protocols.
