# Management Module

Shell-less node management API for Graphene nodes, inspired by Talos Linux's API-only approach.

## Overview

Since Graphene nodes run as hardened unikernels with no shell access, all management operations are performed via a structured API over Iroh QUIC connections.

```
Operator Workstation                    Graphene Node
┌─────────────────────────┐            ┌──────────────────────────┐
│  graphenectl CLI        │            │  ManagementHandler       │
│  + capability token     │───QUIC───→│  - Token validation      │
└─────────────────────────┘            │  - Request processing    │
                                       └──────────────────────────┘
```

**Protocol**: `graphene-mgmt/1` (ALPN identifier)

## Module Structure

| File | Description |
|------|-------------|
| `mod.rs` | Module root, exports public API |
| `protocol.rs` | Request/response types (serde-serializable) |
| `handler.rs` | Request processing and state management |
| `capability.rs` | HMAC-SHA256 token authentication |
| `config.rs` | Node configuration schema |

## Request Types

**Configuration**:
- `ApplyConfig` - Deploy new node configuration
- `GetConfig` - Retrieve current configuration

**Status & Monitoring**:
- `GetStatus` - Node health and worker state
- `StreamLogs` - Real-time log streaming
- `GetMetrics` - Performance metrics snapshot

**Worker Lifecycle** (per WHITEPAPER Section 12.4):
- `Register` / `Unregister` - On-chain registration
- `Join` - Enter worker pool
- `Drain` / `Undrain` - Graceful maintenance mode

**Maintenance**:
- `Upgrade` / `ApplyUpgrade` - OS image updates
- `Reboot` - Node restart

**Capabilities**:
- `GenerateCapability` - Create new auth token
- `RevokeCapability` - Invalidate token
- `ListCapabilities` - Show revoked tokens

## Capability-Based Security

Tokens use HMAC-SHA256 signatures derived from the node's secret key.

**Token Format**:
```
graphene-cap:v1:<role>:<created>:<expires>:<signature>
```

**Role Hierarchy** (higher includes lower permissions):

| Role | Permissions |
|------|-------------|
| `Admin` | Generate/revoke capabilities, reboot, apply upgrades |
| `Operator` | Apply config, register, drain, upgrade |
| `Reader` | Get config/status/metrics, stream logs |

## Worker State Machine

```
UNREGISTERED → REGISTERED → ONLINE ↔ DRAINING
                              ↓
                           UNBONDING → EXITED
```

State transitions are validated by the handler to prevent invalid operations.

## Configuration Schema

```toml
[network]
listen_addr = "0.0.0.0:9000"
advertise_addr = "203.0.113.50:9000"

[staking]
wallet_path = "/etc/graphene/wallet.json"
auto_register = true
stake_amount = 100

[resources]
max_vcpu = 8
max_memory_mb = 16384

[pricing]
cpu_ms_micros = 10.0        # $0.00001 per CPU-ms
memory_mb_ms_micros = 1.0   # $0.000001 per MB-ms
egress_mb_micros = 50.0     # $0.00005 per MB egress

[logging]
level = "info"
format = "json"
```

## Usage

The management API is consumed by `graphenectl` (see `crates/ctl/`):

```bash
# Get node status
graphenectl --node prod-1 status

# Apply configuration
graphenectl --node prod-1 apply -f node-config.toml

# Enter maintenance mode
graphenectl --node prod-1 drain
```

## References

- [WHITEPAPER.md](../../../../docs/WHITEPAPER.md) Section 12.4 - Worker Lifecycle
- [crates/ctl/](../../../ctl/) - Management CLI
- [node-os/](../../../../node-os/) - Host OS without shell
