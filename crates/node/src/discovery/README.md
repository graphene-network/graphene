# Worker Discovery Module

Worker discovery via Iroh gossip for the Graphene network.

## Overview

Workers announce their capabilities, pricing, and load status via gossip. Clients query for suitable workers matching their job requirements.

## Usage

```rust
use monad_node::discovery::{IrohWorkerDiscovery, DiscoveryConfig, JobRequirements};
use monad_node::p2p::{GrapheneNode, P2PConfig};

// Create P2P node and discovery service
let node = Arc::new(GrapheneNode::new(P2PConfig::default()).await?);
let discovery = IrohWorkerDiscovery::new(node.clone(), DiscoveryConfig::default());

// Set our worker announcement (if we're a worker)
discovery.set_announcement(WorkerAnnouncement {
    node_id: node.node_id(),
    version: "0.1.0".into(),
    capabilities: WorkerCapabilities {
        max_vcpu: 8,
        max_memory_mb: 16384,
        kernels: vec!["node-20-unikraft".into()],
    },
    pricing: WorkerPricing {
        cpu_ms_micros: 1,
        memory_mb_ms_micros: 0.1,
    },
    load: WorkerLoad {
        available_slots: 4,
        queue_depth: 0,
    },
    state: GossipWorkerState::Online,
    timestamp: unix_now(),
}).await;

// Start discovery (subscribes to gossip, starts announcer)
discovery.start().await?;

// Find workers matching requirements
let workers = discovery.find_workers(&JobRequirements {
    vcpu: 4,
    memory_mb: 8192,
    kernel: "node-20-unikraft".into(),
    max_price_cpu_ms: Some(10),
}).await;

// Update load when accepting jobs
discovery.update_load(WorkerLoad {
    available_slots: 3,
    queue_depth: 1,
}).await?;

// Graceful shutdown (announces zero slots)
discovery.stop().await?;
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    IrohWorkerDiscovery                      │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌──────────────────┐    │
│  │  Announcer  │  │  Listener   │  │  Expiry Checker  │    │
│  │  (30s loop) │  │  (gossip)   │  │  (cleanup loop)  │    │
│  └──────┬──────┘  └──────┬──────┘  └────────┬─────────┘    │
│         │                │                   │              │
│         ▼                ▼                   ▼              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              known_workers: HashMap                  │   │
│  │                PublicKey → WorkerInfo                │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │   Iroh Gossip Topic    │
              │  "graphene-compute-v1" │
              └────────────────────────┘
```

## Message Types

### WorkerAnnouncement
Full worker info broadcast periodically (default: 30s).

### WorkerHeartbeat
Lightweight load update sent when `update_load()` is called.

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `announce_interval` | 30s | Time between full announcements |
| `heartbeat_interval` | 30s | Time between heartbeats |
| `offline_threshold` | 5 min | Mark worker offline after no updates |
| `expiry_threshold` | 1 hour | Remove worker from list entirely |

## Testing

Use `MockWorkerDiscovery` for unit tests:

```rust
let mock = MockWorkerDiscovery::new();
mock.inject_worker(worker_info);

let found = mock.find_workers(&requirements).await;
assert_eq!(found.len(), 1);

// Check spy state
assert!(mock.spy().start_called);
```

## Files

- `mod.rs` - `WorkerDiscovery` trait, `DiscoveryError`
- `types.rs` - `WorkerInfo`, `JobRequirements`, `DiscoveryConfig`
- `service.rs` - `IrohWorkerDiscovery` implementation
- `mock.rs` - `MockWorkerDiscovery` for testing
