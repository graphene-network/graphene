# ADR-0003: Embedded Relay Service Architecture

**Status:** Proposed
**Date:** 2026-02-06
**Authors:** Marcus
**Relates to:** #140 (Discovery gateway service), #159 (Embedded relay service architecture)

## Context

The OpenCapsule network requires two infrastructure services for SDK clients:

1. **Discovery** - REST API for querying available workers (see ADR-0001)
2. **Relay** - QUIC relay for NAT traversal when direct connections fail

Currently these are conceived as separate services. However, both serve SDK clients and could share infrastructure.

Iroh provides relay functionality via the `iroh-relay` crate, which can be embedded into other services or run standalone.

## Decision

We will **embed the Iroh relay into the Discovery Gateway**, creating a single well-known endpoint that provides both discovery and relay services.

Workers will participate in **opportunistic relaying** via standard Iroh mesh behavior, but will NOT run dedicated relay or discovery API services.

### Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Discovery Gateway                                │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐             │
│  │   REST API  │    │   Gossip    │    │   Worker    │             │
│  │  /workers   │    │  Listener   │    │  Registry   │             │
│  │  /session   │    │             │    │             │             │
│  └─────────────┘    └─────────────┘    └─────────────┘             │
│         │                  ▲                  ▲                      │
│         │                  │                  │                      │
│  ┌──────┴──────────────────┴──────────────────┴─────────────────┐  │
│  │                    Iroh Endpoint                               │  │
│  │              (relay server embedded)                          │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                         QUIC/HTTPS                                   │
└──────────────────────────────┼───────────────────────────────────────┘
                               │
         ┌─────────────────────┼─────────────────────┐
         │                     │                     │
    ┌────▼────┐           ┌────▼────┐           ┌────▼────┐
    │   SDK   │           │ Worker  │           │ Worker  │
    │ Client  │           │    A    │           │    B    │
    └─────────┘           └─────────┘           └─────────┘
```

### Component Responsibilities

| Component | Discovery API | Dedicated Relay | Opportunistic Relay |
|-----------|--------------|-----------------|---------------------|
| **Gateway** | Yes | Yes | - |
| **Worker** | No | No | Yes (Iroh default) |

### Gateway Responsibilities

1. **Discovery REST API** - Aggregate worker announcements from gossip, expose via REST
2. **Session tokens** - Issue JWTs for serverless optimization (see ADR-0001)
3. **Dedicated relay** - Always-available relay endpoint for NAT traversal
4. **Health/metrics** - Operational visibility

### Worker Responsibilities

1. **Job execution** - Primary purpose
2. **Gossip announcements** - Advertise capabilities and availability
3. **Opportunistic relay** - Standard Iroh behavior, helps mesh connectivity

Workers do NOT:
- Run discovery API (they're ephemeral, not well-known endpoints)
- Run dedicated relay (resources should go to job execution)

## Rationale

### Why Combine Gateway + Relay?

1. **Single well-known endpoint** - SDK clients need one reliable server for bootstrapping. Adding relay means they can also use it for NAT traversal without separate configuration.

2. **Already connected to the network** - The gateway subscribes to gossip, so it's already an Iroh endpoint. Embedding relay is minimal overhead.

3. **Operational simplicity** - One service to deploy, monitor, and scale instead of two.

4. **SDK simplification** - Clients configure one URL for both discovery and relay.

### Why NOT Add Discovery/Relay to Workers?

1. **Discovery needs stability** - Clients need a well-known endpoint to bootstrap. Workers come and go (ephemeral by design).

2. **Separation of concerns** - Workers execute jobs, gateways handle network coordination.

3. **Resource isolation** - Dedicated relay traffic shouldn't compete with job execution on workers.

4. **Gossip IS decentralized discovery** - Workers announce via gossip, gateway aggregates for REST convenience. Adding REST API to workers would be redundant.

### Opportunistic Relay on Workers

Workers participate in Iroh's standard mesh networking. When Worker A can't directly reach Worker B, nearby workers may help relay traffic. This is Iroh's default behavior and requires no additional implementation.

This differs from **dedicated relay**:
- Dedicated: Always available, advertised endpoint, accepts relay traffic from anyone
- Opportunistic: Best-effort, helps nearby nodes, no guarantee of availability

## Implementation

### Gateway Changes

Add `iroh-relay` dependency with `server` feature:

```toml
[dependencies]
iroh-relay = { version = "0.96", features = ["server"] }
```

Embed relay server alongside REST:

```rust
use iroh_relay::server::{Server, ServerConfig};

async fn start_gateway(config: GatewayConfig) -> Result<()> {
    // Start Iroh endpoint with embedded relay
    let relay_config = ServerConfig {
        addr: config.relay_addr,
        tls: config.tls_config,
        // ...
    };

    let relay_server = Server::spawn(relay_config).await?;

    // Start REST API server
    let rest_server = start_rest_api(config.rest_addr).await?;

    // Start gossip listener
    let gossip = start_gossip_listener().await?;

    // ...
}
```

### SDK Configuration

Clients configure a single gateway URL:

```typescript
const client = await Client.create({
  gatewayUrl: 'https://gateway.opencapsule.dev',
  // Gateway provides both discovery API and relay
});
```

The SDK:
1. Queries `GET /workers` for discovery
2. Uses gateway as Iroh relay for NAT traversal
3. Establishes direct QUIC connections to workers when possible

### Deployment

Deploy multiple gateway instances across regions for redundancy:

```
gateway-us-east.opencapsule.dev  → US East
gateway-eu-west.opencapsule.dev  → EU West
gateway-ap-south.opencapsule.dev → Asia Pacific
```

SDKs can be configured with multiple gateways for failover:

```typescript
const client = await Client.create({
  gateways: [
    'https://gateway-us-east.opencapsule.dev',
    'https://gateway-eu-west.opencapsule.dev',
  ],
});
```

## Alternatives Considered

### 1. Separate Gateway and Relay Services (Rejected)

Run discovery gateway and relay as separate services.

**Pros:** Cleaner separation of concerns
**Cons:**
- Two services to deploy and maintain
- SDK needs two URLs configured
- Both need similar infrastructure (well-connected, highly available)

### 2. Relay on Every Worker (Rejected)

Have every worker run a dedicated relay.

**Pros:** Fully decentralized
**Cons:**
- Workers should dedicate resources to compute
- Relay traffic competes with job execution
- Most workers behind NAT anyway (can't be relays)

### 3. Use n0.computer Managed Relays (Deferred)

Use Iroh's managed relay service instead of self-hosting.

**Pros:** Less infrastructure to manage
**Cons:**
- External dependency
- No control over availability/performance
- May be used as fallback option

## Consequences

### Positive

- Single well-known endpoint simplifies SDK configuration
- Reduced operational complexity (one service vs two)
- Gateway already has infrastructure requirements (reliable, well-connected)
- Natural fit since both serve SDK clients

### Negative

- Gateway becomes more critical (single point of failure if only one deployed)
- Slightly more complex gateway implementation
- Relay traffic adds load to gateway

### Mitigations

- Deploy multiple gateway instances across regions
- Gateway is stateless, easy to scale horizontally
- Monitor relay traffic separately for capacity planning
- n0.computer managed relays as fallback option

## References

- Iroh relay documentation: https://docs.iroh.computer/deployment/dedicated-infrastructure
- iroh-relay crate: https://github.com/n0-computer/iroh/tree/main/iroh-relay
- ADR-0001: SDK Discovery Architecture
- GitHub Issue #140: Discovery gateway service
- GitHub Issue #159: Embedded relay service architecture
