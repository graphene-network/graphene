# graphenectl

Remote management CLI for Graphene nodes.

## Overview

Since Graphene nodes run as hardened unikernels with no shell access (no SSH, no bash), `graphenectl` provides API-based management over Iroh QUIC connections.

```bash
graphenectl --node prod-1 status
graphenectl --node prod-1 apply -f node-config.toml
graphenectl --node prod-1 drain
```

## Installation

```bash
cargo install --path crates/ctl
```

## Configuration

Node credentials are stored in `~/.graphene/config`:

```yaml
nodes:
  prod-1:
    node_id: "ed25519:abc123..."
    capability: "graphene-cap:v1:operator:..."
    endpoint: "203.0.113.50:9000"

  prod-2:
    node_id: "ed25519:def456..."
    capability: "graphene-cap:v1:admin:..."
```

## Commands

### Bootstrap

Get initial credentials from a new node:

```bash
graphenectl bootstrap --nodes 192.168.1.100:9000
```

### Configuration

```bash
# Apply configuration from file
graphenectl --node prod-1 apply -f node-config.toml

# Get current configuration
graphenectl --node prod-1 get config

# Edit configuration interactively
graphenectl --node prod-1 edit config
```

### Status & Monitoring

```bash
# Get node status
graphenectl --node prod-1 status

# Watch status continuously
graphenectl --node prod-1 status --watch

# Stream logs
graphenectl --node prod-1 logs --follow

# Get metrics snapshot
graphenectl --node prod-1 metrics
```

### Worker Lifecycle

```bash
# Register on-chain with stake
graphenectl --node prod-1 register --stake 100

# Join the worker pool
graphenectl --node prod-1 join

# Enter maintenance mode (stop accepting new jobs)
graphenectl --node prod-1 drain

# Exit maintenance mode
graphenectl --node prod-1 undrain

# Unregister from the network
graphenectl --node prod-1 unregister
```

### Maintenance

```bash
# Check for available upgrades
graphenectl --node prod-1 upgrade

# Apply upgrade (downloads and stages OS image)
graphenectl --node prod-1 upgrade --image https://releases.graphene.network/...

# Reboot node
graphenectl --node prod-1 reboot
```

### Capability Management

```bash
# Generate new capability token
graphenectl --node prod-1 cap generate --role operator --ttl 30

# List revoked capabilities
graphenectl --node prod-1 cap list

# Revoke a capability
graphenectl --node prod-1 cap revoke <token-prefix>
```

### Local Configuration

```bash
# Add node to local config
graphenectl config add prod-3 --node-id ed25519:... --capability graphene-cap:...

# Remove node from local config
graphenectl config remove prod-3

# List configured nodes
graphenectl config list
```

## Global Options

| Option | Description |
|--------|-------------|
| `--config <PATH>` | Config file (default: `~/.graphene/config`) |
| `--node <NAME>` | Target node name or ID |
| `--output <FORMAT>` | Output format: `json`, `yaml`, `text` |
| `-v, --verbose` | Enable verbose logging |

## Roles

Capability tokens have roles that determine allowed operations:

| Role | Allowed Operations |
|------|-------------------|
| `admin` | All operations including reboot, capability management |
| `operator` | Configuration, registration, drain/undrain, upgrades |
| `reader` | Status, logs, metrics (read-only) |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ graphenectl CLI                                             │
│ ┌─────────────────┐  ┌─────────────────┐  ┌──────────────┐ │
│ │ commands/       │  │ client.rs       │  │ config.rs    │ │
│ │ - apply.rs      │  │ ManagementClient│  │ ClientConfig │ │
│ │ - status.rs     │→ │ - request()     │  │ - load()     │ │
│ │ - drain.rs      │  │ - connect()     │  │ - save()     │ │
│ │ - ...           │  └────────┬────────┘  └──────────────┘ │
│ └─────────────────┘           │                             │
└───────────────────────────────┼─────────────────────────────┘
                                │ Iroh QUIC
                                │ ALPN: "graphene-mgmt/1"
                                ▼
                    ┌───────────────────────┐
                    │ Graphene Node         │
                    │ ManagementHandler     │
                    └───────────────────────┘
```

## References

- [Management Module](../node/src/management/) - Server-side implementation
- [WHITEPAPER.md](../../docs/WHITEPAPER.md) Section 12.4 - Worker Lifecycle
- [node-os/](../../node-os/) - Host OS (shell-less)
