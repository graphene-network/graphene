# Graphene Worker

The `graphene-worker` binary runs a compute worker node for the Graphene network.

## Quick Start

```bash
# Build the worker
cargo build -p monad_node --bin graphene-worker

# Copy and edit the example config
cp worker.toml.example worker.toml
# Edit worker.toml with your settings

# Run the worker
./target/debug/graphene-worker run
```

## CLI Commands

```
graphene-worker [OPTIONS] <COMMAND>

Commands:
  run         Start the worker daemon
  register    Register this worker on-chain
  unregister  Unregister this worker and reclaim stake
  status      Show worker status
  version     Show version information

Options:
  -c, --config <CONFIG>  Path to config file [default: worker.toml]
```

### Run

Start the worker daemon:

```bash
graphene-worker run --foreground
```

The worker will:
1. Initialize P2P networking via Iroh
2. Subscribe to the compute gossip topic
3. Broadcast availability announcements
4. Send periodic heartbeats (every 30s)
5. Listen for incoming job requests

Press Ctrl+C for graceful shutdown.

### Register

Register the worker on-chain with a stake:

```bash
# Preview (no transaction)
graphene-worker register --stake 0.1

# Confirm registration
graphene-worker register --stake 0.1 --yes
```

### Unregister

Unregister and reclaim your stake:

```bash
# Preview
graphene-worker unregister

# Confirm
graphene-worker unregister --yes
```

### Status

Check worker registration status:

```bash
# Human-readable output
graphene-worker status

# JSON output
graphene-worker status --format json
```

## Configuration

Create a `worker.toml` file (see `worker.toml.example`):

```toml
[worker]
name = "my-worker"
capabilities = ["python", "cpu"]
price_per_unit = 1000       # lamports per compute unit
max_duration_secs = 300     # max job duration
job_slots = 4               # concurrent job limit

[p2p]
storage_path = ".graphene-worker"
use_relay = true
bind_port = 0               # 0 = random port

[solana]
rpc_url = "https://api.devnet.solana.com"
keypair_path = "~/.config/solana/id.json"
program_id = "DHn6uXWDxnBJpkBhBFHiPoDe3S59EnrRQ9qb5rYUdHEs"

[vmm]
firecracker_path = "firecracker"
runtime_dir = "/tmp/graphene-worker"
default_vcpu = 2
default_memory_mib = 512

[logging]
level = "info"              # trace, debug, info, warn, error
format = "pretty"           # pretty, json, compact
```

### Environment Variables

- `GRAPHENE_CONFIG`: Override config file path
- `RUST_LOG`: Override log level (e.g., `RUST_LOG=debug`)

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    graphene-worker                       │
├─────────────────────────────────────────────────────────┤
│  CLI (clap)                                             │
│    └── run / register / unregister / status             │
├─────────────────────────────────────────────────────────┤
│  Daemon                                                 │
│    ├── State Machine (WorkerStateMachine)               │
│    ├── P2P Loop (gossip events)                         │
│    ├── Heartbeat Loop (30s interval)                    │
│    └── Signal Handler (SIGINT/SIGTERM)                  │
├─────────────────────────────────────────────────────────┤
│  P2P (GrapheneNode)          │  Solana (SolanaClient)   │
│    ├── Iroh endpoint         │    ├── RPC client        │
│    ├── Blob storage          │    ├── register_worker   │
│    └── Gossip messaging      │    └── get_worker_status │
└─────────────────────────────────────────────────────────┘
```

## State Machine

The worker lifecycle is managed by `WorkerStateMachine` (see Whitepaper Section 12.4).

### State Diagram

```
UNREGISTERED → REGISTERED → ONLINE ⟷ BUSY
                              ↓
                          DRAINING
                              ↓
                          UNBONDING
                              ↓
                           EXITED

ONLINE/BUSY ⟷ OFFLINE (connection loss/reconnect)
```

### States

| State | Description |
|-------|-------------|
| `Unregistered` | Initial state before Solana registration |
| `Registered` | Stake confirmed, awaiting P2P gossip join |
| `Online` | Active and accepting jobs (has available slots) |
| `Busy` | Active but at capacity (no available slots) |
| `Draining` | Graceful shutdown initiated, finishing current jobs |
| `Offline` | Temporarily disconnected from P2P network |
| `Unbonding` | Unbonding period active (14-day cooldown) |
| `Exited` | Terminal state, worker has exited |

### Events

| Event | Transition |
|-------|------------|
| `StakeConfirmed` | Unregistered → Registered |
| `JoinedGossip` | Registered → Online |
| `SlotsFull` | Online → Busy |
| `SlotAvailable` | Busy → Online |
| `ShutdownRequested` | Online/Busy → Draining |
| `AllJobsComplete` | Draining → Unbonding |
| `UnbondingComplete` | Unbonding → Exited |
| `ConnectionLost` | Online/Busy → Offline |
| `Reconnected` | Offline → Online/Busy |

### Slot Management

Job slots are managed with RAII via `SlotGuard`:

```rust
let state_machine = WorkerStateMachine::new_shared(4);

// Reserve a slot (returns SlotGuard)
if let Ok(guard) = state_machine.try_reserve_slot() {
    // Slot is reserved, state may transition to Busy
    process_job().await;
    // Guard dropped here, slot released, may transition back to Online
}
```

The state machine automatically transitions between `Online` and `Busy` based on slot availability.

## Gossip Protocol

Workers communicate on two gossip topics:

### compute_v1 (`graphene-compute-v1`)

- `WorkerAnnouncement`: Broadcast on startup with capabilities and pricing
- `WorkerHeartbeat`: Periodic liveness signal with load metrics
- `DiscoveryQuery`: Client queries for matching workers
- `DiscoveryResponse`: Worker responses to discovery

### tickets_v1 (`graphene-tickets-v1`)

- `TicketAccepted`: Worker claims a payment ticket
- `TicketRejected`: Double-spend detection

## Solana Integration

The worker interacts with the Graphene Solana program for:

- **Registration**: Create on-chain worker account with stake
- **Unregistration**: Close account and reclaim stake
- **Status**: Query registration state

PDA derivation: `[b"worker", authority_pubkey]`

## Module Structure

```
worker/
├── mod.rs      # Worker struct, stats, exports
├── config.rs   # WorkerConfig TOML parsing
├── daemon.rs   # run_daemon, signal handling
├── error.rs    # WorkerError enum
├── solana.rs   # SolanaClient
└── state.rs    # WorkerStateMachine, SlotGuard
```

## Future Work

- Job execution via Firecracker VMM
- Heartbeat loop with actual load metrics (#45)
- Payment channel settlement
- Integrate state machine with actual Solana stake confirmation
