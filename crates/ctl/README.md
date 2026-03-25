# opencapsulectl

Remote management CLI for OpenCapsule nodes.

## Overview

Since OpenCapsule nodes run as hardened unikernels with no shell access (no SSH, no bash), `opencapsulectl` provides API-based management over Iroh QUIC connections.

```bash
opencapsulectl --node prod-1 status
opencapsulectl --node prod-1 apply -f node-config.toml
opencapsulectl --node prod-1 drain
```

## Installation

```bash
cargo install --path crates/ctl
```

## Configuration

Node credentials are stored in `~/.opencapsule/config`:

```yaml
nodes:
  prod-1:
    node_id: "ed25519:abc123..."
    capability: "opencapsule-cap:v1:operator:..."
    endpoint: "203.0.113.50:9000"

  prod-2:
    node_id: "ed25519:def456..."
    capability: "opencapsule-cap:v1:admin:..."
```

## Commands

### Bootstrap

Get initial credentials from a new node:

```bash
opencapsulectl bootstrap --nodes 192.168.1.100:9000
```

### Configuration

```bash
# Apply configuration from file
opencapsulectl --node prod-1 apply -f node-config.toml

# Get current configuration
opencapsulectl --node prod-1 get config

# Edit configuration interactively
opencapsulectl --node prod-1 edit config
```

### Status & Monitoring

```bash
# Get node status
opencapsulectl --node prod-1 status

# Watch status continuously
opencapsulectl --node prod-1 status --watch

# Stream logs
opencapsulectl --node prod-1 logs --follow

# Get metrics snapshot
opencapsulectl --node prod-1 metrics
```

### Worker Lifecycle

```bash
# Register on-chain with stake
opencapsulectl --node prod-1 register --stake 100

# Join the worker pool
opencapsulectl --node prod-1 join

# Enter maintenance mode (stop accepting new jobs)
opencapsulectl --node prod-1 drain

# Exit maintenance mode
opencapsulectl --node prod-1 undrain

# Unregister from the network
opencapsulectl --node prod-1 unregister
```

### Maintenance

```bash
# Check for available upgrades
opencapsulectl --node prod-1 upgrade

# Apply upgrade (downloads and stages OS image)
opencapsulectl --node prod-1 upgrade --image https://releases.opencapsule.dev/...

# Reboot node
opencapsulectl --node prod-1 reboot
```

### Capability Management

```bash
# Generate new capability token
opencapsulectl --node prod-1 cap generate --role operator --ttl 30

# List revoked capabilities
opencapsulectl --node prod-1 cap list

# Revoke a capability
opencapsulectl --node prod-1 cap revoke <token-prefix>
```

### Local Configuration

```bash
# Add node to local config
opencapsulectl config add prod-3 --node-id ed25519:... --capability opencapsule-cap:...

# Remove node from local config
opencapsulectl config remove prod-3

# List configured nodes
opencapsulectl config list
```

## Global Options

| Option | Description |
|--------|-------------|
| `--config <PATH>` | Config file (default: `~/.opencapsule/config`) |
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
│ opencapsulectl CLI                                             │
│ ┌─────────────────┐  ┌─────────────────┐  ┌──────────────┐ │
│ │ commands/       │  │ client.rs       │  │ config.rs    │ │
│ │ - apply.rs      │  │ ManagementClient│  │ ClientConfig │ │
│ │ - status.rs     │→ │ - request()     │  │ - load()     │ │
│ │ - drain.rs      │  │ - connect()     │  │ - save()     │ │
│ │ - ...           │  └────────┬────────┘  └──────────────┘ │
│ └─────────────────┘           │                             │
└───────────────────────────────┼─────────────────────────────┘
                                │ Iroh QUIC
                                │ ALPN: "opencapsule-mgmt/1"
                                ▼
                    ┌───────────────────────┐
                    │ OpenCapsule Node         │
                    │ ManagementHandler     │
                    └───────────────────────┘
```

## References

- [Management Module](../node/src/management/) - Server-side implementation
- [WHITEPAPER.md](../../docs/WHITEPAPER.md) Section 12.4 - Worker Lifecycle
- [node-os/](../../node-os/) - Host OS (shell-less)
