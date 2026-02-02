1. Schema Specification
The Address Book is stored as an Iroh Replica. Each entry in the document uses the following structure:

Key Format

We use a hierarchical key structure to allow for efficient prefix-based queries (e.g., "Give me all nodes in North America").

Component	Description	Example
Prefix	Static namespace	nodes/
Region	Geographic or logical grouping	us-east/
NodeID	The Ed25519 Public Key (z32 encoded)	z52...x8
Full Key Example: nodes/us-east/z52f8q7...

Value Payload (JSON)

The value associated with each key is a JSON-serialized blob containing the node's "Identity Card."

JSON
{
  "version": 1,
  "node_id": "z52...",
  "capabilities": {
    "vcpus": 4,
    "ram_gb": 16,
    "arch": "x86_64",
    "features": ["kvm", "buildkit", "gpu-a100"]
  },
  "endpoints": {
    "home_relay": "https://use1-1.derp.iroh.network",
    "direct_addresses": ["1.2.3.4:12345"]
  },
  "status": "active",
  "timestamp": 1706869500
}
2. Protocol Mechanisms
To maintain this book, the network utilizes three specific Iroh sub-protocols:

A. Range-Set Reconciliation (iroh-docs)

When a node joins the swarm, it doesn't download the whole book. It performs a Willow reconciliation. It compares fingerprints of key ranges with its neighbors. If a neighbor has an update in the nodes/ range, the node pulls only the specific "deltas" (new entries or updates).

B. Last-Write-Wins (LWW) Conflicts

Since multiple nodes might try to update an entry, Iroh-Docs uses LWW semantics based on the Author ID.

If Node A updates its own metadata, its signature (AuthorId) proves ownership.

If two updates occur, the one with the higher Lamport Timestamp (or wall clock if synchronized) becomes the canonical truth.

C. Gossip Propagation

While the "Doc" is for persistence, iroh-gossip is for the "Live View."

When a node updates its status in the Doc, a gossip signal is sent to the topic.

Peers receive the signal and immediately trigger a sync for that specific key in the Doc.

3. Rust Implementation Spec
Core Data Structure

Rust
use iroh::docs::{Doc, Entry};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct nodeEntry {
    pub capabilities: Capabilities,
    pub status: nodeStatus,
    pub last_update: u64,
}

pub struct TalosRegistry {
    // The shared Iroh Document
    doc: Doc,
    // The secret key used to sign our own updates
    author: iroh::docs::AuthorId,
}
The "Heartbeat" Loop

Every worker node must run a background task to keep its entry "fresh."

Rust
async fn heartbeat_task(registry: TalosRegistry, my_meta: nodeEntry) {
    let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 mins
    loop {
        interval.tick().await;
        let key = format!("nodes/default/{}", my_node_id);
        registry.doc.set_json(author, key, &my_meta).await.unwrap();
        println!("💓 Heartbeat synced to global address book.");
    }
}
4. Why this works for Talos
Security: Every entry is signed by an AuthorId. It is impossible for Node B to overwrite Node A's address book entry because they don't have Node A's private key.

Scalability: Because of prefix queries, an node looking for a job doesn't need to load the entire 10,000-node network. It can just sync nodes/my-region/*.

Reliability: If the user’s internet cuts out for 10 minutes, their address book is still there when they reconnect—Iroh-Docs just picks up where it left off.