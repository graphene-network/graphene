# ADR-0001: SDK Discovery Architecture

**Status:** Proposed
**Date:** 2026-02-04
**Authors:** Marcus
**Relates to:** #68 (TypeScript SDK), #50 (Worker selection), #133 (Gossip-based discovery)

## Context

The current SDK implementation requires clients to specify a `workerNodeId` at client creation time:

```typescript
const client = await Client.create({
  secretKey,
  channelPda,
  workerNodeId,  // Must know worker before creating client
});
```

This conflicts with the whitepaper vision (Section 5.2, 13.5) where discovery is implicit:

```typescript
const client = new Client();
const result = await client.run({ code: '...' });
```

The gap exists because:
1. Channel keys are derived at client creation using the worker's public key
2. No discovery mechanism is exposed to the SDK
3. The SDK assumes the client already knows which worker to use

## Decision

We will implement a **two-phase architecture** that separates network bootstrap from worker selection:

### Phase 1: Network Client (Bootstrap)

```typescript
interface NetworkConfig {
  secretKey: Uint8Array;           // Client identity
  storagePath?: string;            // Iroh state persistence
  relays?: string[];               // DERP relay URLs
  discoveryMode: 'gateway' | 'p2p';
  discoveryUrl?: string;           // Gateway URL (if mode=gateway)
}

const network = await Network.create(config);
```

The `Network` client:
- Initializes Iroh P2P endpoint
- Connects to relay network for NAT traversal
- Does NOT derive channel keys (no worker selected yet)

### Phase 2: Worker Discovery

```typescript
interface WorkerFilter {
  minVcpu?: number;
  minMemoryMb?: number;
  kernel?: string;
  regions?: string[];               // e.g., ['us-*', 'eu-west-*']
  maxPriceCpuMs?: number;           // microtokens
  minReputation?: number;           // 0.0-1.0 success rate
}

const workers = await network.discoverWorkers(filter);
// Returns: WorkerInfo[] with capabilities, pricing, load, reputation
```

Discovery supports two modes:

**Gateway Mode** (default, lightweight):
- SDK queries REST endpoint: `GET /workers?kernel=python:3.12&region=us-*`
- Gateway aggregates gossip announcements
- Works in browsers, mobile, serverless functions

**P2P Mode** (advanced, fully decentralized):
- SDK joins `graphene-compute-v1` gossip topic
- Maintains local worker registry
- Suitable for long-running services

### Phase 3: Channel Opening (Key Derivation)

```typescript
const channel = await network.openChannel({
  worker: workers[0],        // Selected worker
  channelPda: myChannelPda,  // Solana payment channel
});

// Channel keys derived HERE, after worker selection
```

### Phase 4: Job Execution

```typescript
const result = await channel.run({
  code: 'print(2 + 2)',
  kernel: 'python:3.12',
});
```

### Simplified API (Implicit Discovery)

For the common case, we provide a convenience wrapper:

```typescript
const client = new Client();  // Uses gateway, auto-discovers
const result = await client.run({ code: '...' });

// Equivalent to:
// 1. Network.create({ discoveryMode: 'gateway' })
// 2. discoverWorkers({ kernel: inferred })
// 3. openChannel({ worker: bestMatch })
// 4. channel.run({ code })
```

## Discovery Gateway Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Discovery Gateway                            │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│  │   REST API  │    │   Gossip    │    │   Worker    │         │
│  │  /workers   │◄───│  Listener   │◄───│  Registry   │         │
│  └─────────────┘    └─────────────┘    └─────────────┘         │
│                            ▲                                     │
└────────────────────────────┼─────────────────────────────────────┘
                             │ graphene-compute-v1
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐         ┌────▼────┐         ┌────▼────┐
    │ Worker  │         │ Worker  │         │ Worker  │
    │   A     │         │   B     │         │   C     │
    └─────────┘         └─────────┘         └─────────┘
```

### Gateway API

```
GET /workers
  ?kernel=python:3.12
  &minVcpu=2
  &minMemoryMb=1024
  &region=us-*
  &maxPrice=100
  &limit=10

Response:
{
  "workers": [
    {
      "nodeId": "abc123...",
      "capabilities": { "maxVcpu": 8, "maxMemoryMb": 16384, ... },
      "pricing": { "cpuMsMicros": 10, "memoryMbMsMicros": 1, ... },
      "load": { "availableSlots": 3, "queueDepth": 0 },
      "regions": [{ "country": "US", "cloudRegion": "us-east-1" }],
      "reputation": { "successRate": 0.99, "jobsCompleted": 1234, ... }
    }
  ],
  "networkStats": {
    "medianPriceCpuMs": 12,
    "totalWorkers": 150,
    "totalAvailableSlots": 423
  }
}
```

## Key Derivation Timing

**Before (Current - Incorrect):**
```
Client.create(config)
  ├─ Parse workerNodeId           ← Must know worker
  ├─ Derive channel keys          ← Keys derived too early
  └─ Initialize P2P
```

**After (Proposed - Correct):**
```
Network.create(config)
  └─ Initialize P2P only          ← No worker, no keys

network.discoverWorkers(filter)
  └─ Query gateway or gossip      ← Find available workers

network.openChannel({ worker, channelPda })
  ├─ Derive channel keys          ← Keys derived HERE
  └─ Return Channel instance

channel.run(options)
  └─ Execute job with encryption
```

## Alternatives Considered

### 1. Always P2P (Rejected)

Require SDK to join gossip for discovery.

**Pros:** Fully decentralized
**Cons:**
- Heavy for browsers/mobile
- Always-on gossip subscription
- Not suitable for serverless

### 2. Worker Pinning Only (Rejected)

Keep current architecture, require out-of-band worker discovery.

**Pros:** Simple implementation
**Cons:**
- Violates whitepaper UX vision
- Poor developer experience
- No automatic failover

### 3. Centralized Registry (Rejected)

Single authoritative worker registry.

**Pros:** Simple, fast
**Cons:**
- Single point of failure
- Censorship risk
- Doesn't match decentralized ethos

## Implementation Plan

### Phase 1: Discovery Gateway (P1)
1. Create `graphene-discovery` service
2. Subscribe to `graphene-compute-v1` gossip
3. Expose REST API for worker queries
4. Deploy to multiple regions

### Phase 2: SDK Refactor (P1)
1. Split `Client` into `Network` + `Channel`
2. Move key derivation to `openChannel()`
3. Add `discoverWorkers()` method
4. Maintain backwards-compatible `Client` wrapper

### Phase 3: P2P Discovery Mode (P2)
1. Expose `IrohWorkerDiscovery` via napi-rs
2. Add `discoveryMode: 'p2p'` option
3. Local worker registry in SDK

## Consequences

### Positive
- Matches whitepaper vision for simple `new Client()` API
- Enables automatic worker selection and failover
- Supports lightweight clients (browsers, mobile)
- Proper separation of concerns

### Negative
- Gateway introduces semi-centralized component
- More complex SDK architecture
- Breaking change for existing SDK users

### Mitigations
- Run multiple gateway instances (decentralized operation)
- P2P mode available for advanced users
- Deprecation period for old API

## References

- Whitepaper Section 5.2: Job Execution Flow
- Whitepaper Section 13: SDK Quick Start
- GitHub Issue #68: TypeScript SDK
- GitHub Issue #50: Worker selection algorithm
- GitHub Issue #133: Gossip-based node discovery
- Iroh documentation: https://iroh.computer/docs
