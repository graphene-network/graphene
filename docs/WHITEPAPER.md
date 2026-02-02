# Talos Network

**A Zero-Latency Decentralized Serverless Platform**

Version 5.0
February 2026

---

## Abstract

Talos is a decentralized compute network optimized for ephemeral serverless functions and AI inference. By combining **Unikraft unikernels** with **Firecracker MicroVMs**, Talos achieves sub-second cold starts with hardware-level isolation. The network uses **Solana** for settlement, **Iroh** for peer-to-peer data transfer, and **off-chain payment channels** for zero-latency job execution.

Unlike traditional DePIN compute networks that suffer from Docker image bloat and blockchain consensus delays, Talos delivers performance comparable to AWS Lambda while maintaining a permissionless, trustless architecture.

---

## 1. The Problem

Current decentralized compute networks face three structural bottlenecks:

### 1.1 The Container Bottleneck
Shipping gigabyte-sized Docker images for every job creates unacceptable latency. A typical serverless cold start on existing DePIN networks takes 30-120 seconds.

### 1.2 The Consensus Lag
Waiting for blockchain finality before starting execution destroys real-time use cases. Even 400ms of consensus delay is too slow for interactive AI inference or API backends.

### 1.3 The Gas Friction
Requiring users to hold native gas tokens and sign transactions for every job ruins the developer experience and creates unnecessary barriers to adoption.

---

## 2. The Talos Solution

Talos decouples **work** from **settlement**:

- **Work** happens instantly over a P2P network
- **Payment** happens instantly via cryptographic tickets
- **Settlement** happens asynchronously on Solana

### 2.1 Key Innovations

| Innovation | Benefit |
|------------|---------|
| **Unikraft Unikernels** | 1-5MB images instead of gigabytes |
| **Content-Addressable Caching** | 99% cache hit rate for common stacks |
| **Payment Channels** | Zero blockchain latency per job |
| **Ephemeral Builder VMs** | Secure builds without trusting user code |

---

## 3. Architecture

The Talos stack consists of four layers, all implemented in Rust.

```
┌─────────────────────────────────────────────────────────┐
│                   USER / AGENT                          │
└─────────────────────┬───────────────────────────────────┘
                      │ Job Request + Payment Ticket
                      ▼
┌─────────────────────────────────────────────────────────┐
│              LAYER 4: ECONOMIC PLANE                    │
│         Off-chain Payment Channels (Ed25519)            │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│              LAYER 3: DATA PLANE                        │
│              Iroh (QUIC + Gossip)                       │
│    Discovery · NAT Traversal · Blob Transfer            │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│              LAYER 2: EXECUTION PLANE                   │
│         Firecracker MicroVMs + Unikraft                 │
│      Ephemeral Builders · Content-Addressed Cache       │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│              LAYER 1: SETTLEMENT PLANE                  │
│              Solana (Anchor Program)                    │
│       Channel Management · Staking · Slashing           │
└─────────────────────────────────────────────────────────┘
```

### 3.1 Layer 1: Settlement Plane (Solana)

Solana serves as the financial backbone. The Talos Anchor program handles:

- **Payment Channels**: Users lock funds in PDAs (Program Derived Addresses)
- **Worker Registry**: Staked workers with advertised capabilities
- **Settlement**: Batch verification of Ed25519 payment tickets
- **Slashing**: Penalizing misbehaving workers

The blockchain is never in the critical path of job execution. Users open a payment channel once, then execute thousands of jobs without touching the chain.

### 3.2 Layer 2: Execution Plane (Firecracker + Unikraft)

Jobs run in Firecracker MicroVMs containing Unikraft unikernels. This provides:

- **Hardware Isolation**: KVM-based virtualization (no shared kernel)
- **Minimal Attack Surface**: Unikernels contain only required code
- **Fast Boot**: <200ms cold start for cached images

#### The Build Pipeline

Users submit standard Dockerfiles. The network compiles them into minimal unikernels:

1. **Submission**: User uploads `Dockerfile` + `Kraftfile` via Iroh
2. **Ephemeral Builder**: Worker spawns isolated Builder VM (MicroVM-for-building)
3. **Compilation**: BuildKit + Unikraft toolchain produces `.unik` binary
4. **Handoff**: Binary passed to host, Builder VM destroyed
5. **Execution**: `.unik` runs in production MicroVM

The Ephemeral Builder has zero access to host keys, files, or network—preventing `RUN` command exploits.

#### Content-Addressable Caching

Every artifact is content-addressed:

```
cache_key = hash(kernel_version + requirements_hash + code_hash)
```

**Cache Hierarchy:**

| Layer | Contents | Hit Rate | Lookup Time |
|-------|----------|----------|-------------|
| L1 | Kernel | ~100% | <1ms |
| L2 | Dependencies | ~95% | <1ms |
| L3 | User Code | Variable | <1ms |

For popular stacks (Python + Pandas, Node + Express), cold start approaches the theoretical minimum: ~125ms Firecracker boot overhead.

### 3.3 Layer 3: Data Plane (Iroh)

Iroh provides the peer-to-peer networking layer:

- **Gossip Protocol**: Workers announce availability on `talos-compute-v1` topic
- **Magicsock**: NAT traversal via UDP hole-punching and DERP relays
- **QUIC Multiplexing**: Concurrent streams for tickets, code, and results
- **Content-Addressed Blobs**: Verified chunk-by-chunk transfer

Data flows directly between user and worker. The blockchain never sees job payloads.

### 3.4 Layer 4: Economic Plane (Payment Channels)

Zero-latency payments via unidirectional state channels:

1. **Open Channel**: User locks funds on Solana (one-time)
2. **Issue Tickets**: For each job, user signs off-chain payment ticket
3. **Local Verification**: Worker validates Ed25519 signature (<1ms)
4. **Batch Settlement**: Worker submits final ticket to claim accumulated payments

```
┌──────────┐    Ticket (off-chain)    ┌──────────┐
│   USER   │ ──────────────────────▶  │  WORKER  │
└──────────┘                          └────┬─────┘
     │                                     │
     │ Lock Funds (once)                   │ Settle (batch)
     ▼                                     ▼
┌─────────────────────────────────────────────────┐
│                    SOLANA                        │
│              Payment Channel PDA                 │
└─────────────────────────────────────────────────┘
```

---

## 4. Job Lifecycle

### 4.1 Phase 1: Channel Setup (One-Time)

User opens payment channel with worker (or gateway):

```rust
// Solana transaction
open_channel {
    user: Pubkey,
    worker: Pubkey,
    amount: 100 USDC,
    timeout: 7 days
}
```

Funds are locked in a PDA. User receives channel ID.

### 4.2 Phase 2: Job Execution (Real-Time Loop)

**Step 1: Discovery**
User queries gossip network for available workers matching requirements:
- Region preferences
- Resource requirements (vCPU, memory)
- Price constraints

**Step 2: Submission**
User sends job via Iroh direct connection:

```json
{
  "manifest": {
    "code_hash": "blake3:abc123...",
    "deps_hash": "blake3:def456...",
    "entrypoint": "main.py",
    "resources": {
      "vcpu": 2,
      "memory_mb": 2048,
      "max_duration_ms": 30000
    },
    "network": {
      "egress_allowlist": ["api.openai.com"]
    }
  },
  "ticket": {
    "channel_id": "xyz",
    "amount": 0.05,
    "nonce": 42,
    "signature": "ed25519:..."
  },
  "assets": {
    "code_url": "iroh:blob/abc123",
    "input_url": "iroh:blob/def456"
  }
}
```

**Step 3: Verification**
Worker validates locally (<5ms):
- Is ticket signature valid?
- Does channel have sufficient balance?
- Is nonce higher than last seen?

**Step 4: Execution**
Worker assembles and boots MicroVM:
- Check L2 cache for dependencies (instant if hit)
- If miss, build in Ephemeral Builder VM
- Mount kernel + deps + code as block devices
- Boot Firecracker, run entrypoint

**Step 5: Result Delivery**
Worker creates result blob and notifies user:

```json
{
  "job_id": "...",
  "result_hash": "blake3:result789...",
  "exit_code": 0,
  "duration_ms": 4523,
  "signature": "ed25519:..."
}
```

User fetches result blob via Iroh. Result is pinned for 24 hours.

### 4.3 Phase 3: Settlement (Asynchronous)

After accumulating tickets, worker submits final ticket to Solana:

```rust
settle_channel {
    channel_id: "xyz",
    final_amount: 50 USDC,
    final_nonce: 1000,
    user_signature: [u8; 64]
}
```

Anchor program verifies signature via Ed25519 introspection and transfers funds.

### 4.4 Sequence Diagram: Single Job

```
┌──────┐          ┌──────┐          ┌────────┐          ┌────────┐
│ User │          │ Iroh │          │ Worker │          │ Solana │
└──┬───┘          └──┬───┘          └───┬────┘          └───┬────┘
   │                 │                  │                   │
   │  1. Open Channel (one-time)        │                   │
   │────────────────────────────────────────────────────────▶
   │                 │                  │                   │
   │                 │                  │    Lock funds     │
   │                 │                  │◀──────────────────│
   │                 │                  │                   │
   │  2. Discover Workers               │                   │
   │────────────────▶│                  │                   │
   │                 │   Gossip query   │                   │
   │                 │─────────────────▶│                   │
   │                 │   Worker list    │                   │
   │◀────────────────│◀─────────────────│                   │
   │                 │                  │                   │
   │  3. Submit Job + Ticket            │                   │
   │─────────────────────────────────────▶                  │
   │                 │                  │                   │
   │                 │    4. Verify     │                   │
   │                 │    ticket sig    │                   │
   │                 │    (local, <1ms) │                   │
   │                 │                  │                   │
   │                 │    5. Check      │                   │
   │                 │    cache (L2)    │                   │
   │                 │                  │                   │
   │                 │    6. Boot VM    │                   │
   │                 │    + Execute     │                   │
   │                 │                  │                   │
   │  7. Stream Result                  │                   │
   │◀─────────────────────────────────────                  │
   │                 │                  │                   │
   │        [... repeat jobs 3-7 ...]   │                   │
   │                 │                  │                   │
   │                 │  8. Settle (batch, async)            │
   │                 │                  │──────────────────▶│
   │                 │                  │   Verify sig      │
   │                 │                  │   Transfer funds  │
   │                 │                  │◀──────────────────│
   │                 │                  │                   │
```

### 4.5 Sequence Diagram: DAG Workflow

```
┌──────┐       ┌────────┐       ┌────────┐       ┌────────┐
│ User │       │Worker A│       │Worker B│       │Worker C│
└──┬───┘       └───┬────┘       └───┬────┘       └───┬────┘
   │               │               │               │
   │ Submit DAG    │               │               │
   │ (3 jobs)      │               │               │
   │──────────────▶│               │               │
   │               │               │               │
   │               │ Run job_1     │               │
   │               │───────┐       │               │
   │               │       │       │               │
   │               │◀──────┘       │               │
   │               │               │               │
   │               │ Spawn job_2   │               │
   │               │ (distributed) │               │
   │               │──────────────▶│               │
   │               │               │               │
   │               │ Spawn job_3   │               │
   │               │ (distributed) │               │
   │               │──────────────────────────────▶│
   │               │               │               │
   │               │               │ Run job_2     │
   │               │               │───────┐       │
   │               │               │       │       │ Run job_3
   │               │               │◀──────┘       │───────┐
   │               │               │               │       │
   │               │               │               │◀──────┘
   │               │               │               │
   │               │  Result job_2 │               │
   │               │◀──────────────│               │
   │               │               │  Result job_3 │
   │               │◀──────────────────────────────│
   │               │               │               │
   │ Final Result  │               │               │
   │◀──────────────│               │               │
   │               │               │               │
```

### 4.6 Payment Channel State Machine

```
                              ┌─────────────────┐
                              │                 │
            open_channel()    │     CLOSED      │
         ┌───────────────────▶│   (no funds)    │
         │                    │                 │
         │                    └────────┬────────┘
         │                             │
         │                             │ User sends open_channel tx
         │                             │ + deposits funds
         │                             ▼
         │                    ┌─────────────────┐
         │                    │                 │
         │                    │      OPEN       │◀─────────────────┐
         │                    │  (funds locked) │                  │
         │                    │                 │                  │
         │                    └────────┬────────┘                  │
         │                             │                           │
         │              ┌──────────────┼──────────────┐            │
         │              │              │              │            │
         │              ▼              ▼              ▼            │
         │      ┌───────────┐  ┌───────────┐  ┌───────────┐       │
         │      │  Issue    │  │   Top-up  │  │  Timeout  │       │
         │      │  Ticket   │  │  (deposit │  │  Request  │       │
         │      │ (off-chain│  │   more)   │  │           │       │
         │      │  nonce++)  │  └─────┬─────┘  └─────┬─────┘       │
         │      └─────┬─────┘        │              │              │
         │            │              │              ▼              │
         │            │              │      ┌───────────────┐      │
         │            │              │      │               │      │
         │            │              │      │   DISPUTING   │      │
         │            │              │      │  (24h window) │      │
         │            │              │      │               │      │
         │            │              │      └───────┬───────┘      │
         │            │              │              │              │
         │            │              │    ┌─────────┴─────────┐    │
         │            │              │    │                   │    │
         │            │              │    ▼                   ▼    │
         │            │              │  Worker             No      │
         │            │              │  submits            dispute │
         │            │              │  ticket             filed   │
         │            │              │    │                   │    │
         │            ▼              │    ▼                   │    │
         │     ┌─────────────┐       │  ┌─────────────┐      │    │
         │     │   Worker    │       │  │   Settle    │      │    │
         │     │   settles   │───────┘  │  to worker  │──────┘    │
         │     │  (batched)  │          │  (disputed) │           │
         │     └──────┬──────┘          └──────┬──────┘           │
         │            │                        │                   │
         │            ▼                        ▼                   │
         │     ┌─────────────────────────────────────┐            │
         │     │                                     │            │
         └─────│             SETTLED                 │────────────┘
               │      (funds transferred)            │   Reopen
               │                                     │
               └─────────────────────────────────────┘
```

**Channel States:**

| State | Description | Transitions |
|-------|-------------|-------------|
| **CLOSED** | No channel exists | → OPEN (user deposits) |
| **OPEN** | Funds locked, tickets can be issued | → SETTLED (worker settles) |
| | | → DISPUTING (user requests timeout) |
| **DISPUTING** | 24h window for worker to submit tickets | → SETTLED (ticket submitted or timeout) |
| **SETTLED** | Funds distributed, channel closed | → OPEN (reopen with new deposit) |

### 4.7 Job State Machine

```
                    ┌───────────────┐
                    │               │
     Submit job     │   PENDING     │
    ────────────────▶   (queued)    │
                    │               │
                    └───────┬───────┘
                            │
                            │ Worker accepts
                            ▼
                    ┌───────────────┐
                    │               │
                    │   ACCEPTED    │
                    │ (ticket held) │
                    │               │
                    └───────┬───────┘
                            │
              ┌─────────────┴─────────────┐
              │                           │
              ▼                           ▼
      ┌───────────────┐           ┌───────────────┐
      │               │           │               │
      │   BUILDING    │           │   CACHED      │
      │ (deps build)  │           │ (cache hit)   │
      │               │           │               │
      └───────┬───────┘           └───────┬───────┘
              │                           │
              └─────────────┬─────────────┘
                            │
                            ▼
                    ┌───────────────┐
                    │               │
                    │   RUNNING     │
                    │  (VM booted)  │
                    │               │
                    └───────┬───────┘
                            │
          ┌─────────────────┼─────────────────┐
          │                 │                 │
          ▼                 ▼                 ▼
  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
  │               │ │               │ │               │
  │  SUCCEEDED    │ │    FAILED     │ │   TIMEOUT     │
  │  (exit 0)     │ │  (exit 1-127) │ │  (exit 128)   │
  │               │ │               │ │               │
  └───────┬───────┘ └───────┬───────┘ └───────┬───────┘
          │                 │                 │
          └─────────────────┴─────────────────┘
                            │
                            ▼
                    ┌───────────────┐
                    │               │
                    │  DELIVERING   │
                    │ (result blob) │
                    │               │
                    └───────┬───────┘
                            │
              ┌─────────────┴─────────────┐
              │                           │
              ▼                           ▼
      ┌───────────────┐           ┌───────────────┐
      │               │           │               │
      │   DELIVERED   │           │   EXPIRED     │
      │ (user pulled) │           │ (TTL passed)  │
      │               │           │               │
      └───────────────┘           └───────────────┘
```

**Job States:**

| State | Duration | Next States |
|-------|----------|-------------|
| PENDING | <100ms | ACCEPTED |
| ACCEPTED | <10ms | BUILDING, CACHED |
| BUILDING | 1-60s | RUNNING |
| CACHED | <1ms | RUNNING |
| RUNNING | user-defined max | SUCCEEDED, FAILED, TIMEOUT |
| SUCCEEDED | instant | DELIVERING |
| FAILED | instant | DELIVERING |
| TIMEOUT | instant | DELIVERING |
| DELIVERING | <24h | DELIVERED, EXPIRED |
| DELIVERED | terminal | - |
| EXPIRED | terminal | - |

### 4.8 Workflow State Machine

```
                         ┌────────────────┐
                         │                │
        Submit workflow  │    PENDING     │
       ──────────────────▶                │
                         │                │
                         └───────┬────────┘
                                 │
                                 │ Start entry job
                                 ▼
                         ┌────────────────┐
                         │                │◀──────────────────┐
                         │    RUNNING     │                   │
                         │ (jobs active)  │───────────────────┤
                         │                │  Job completes,   │
                         └───────┬────────┘  spawn next jobs  │
                                 │                            │
               ┌─────────────────┼─────────────────┐          │
               │                 │                 │          │
               ▼                 ▼                 ▼          │
       ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
       │              │  │              │  │              │   │
       │  COMPLETED   │  │   FAILED     │  │  PARTIAL     │───┘
       │  (all done)  │  │ (job fault)  │  │ (some done)  │
       │              │  │              │  │              │
       └──────────────┘  └──────────────┘  └──────────────┘
```

**Workflow States:**

| State | Description |
|-------|-------------|
| PENDING | Workflow submitted, not yet started |
| RUNNING | One or more jobs executing |
| COMPLETED | All jobs finished successfully |
| FAILED | A required job failed (workflow aborted) |
| PARTIAL | Fan-out with some successes, some failures |

---

## 5. Tokenomics

### 5.1 The $TALOS Token

$TALOS is an SPL token with two primary functions:

**1. Worker Staking (Security)**
Workers must stake $TALOS proportional to their advertised compute:

| Resource | Stake Required |
|----------|----------------|
| Base | 100 $TALOS |
| Per vCPU | 50 $TALOS |
| Per GB RAM | 10 $TALOS |
| Per GPU | 500 $TALOS |

Example: 8 vCPU, 32GB RAM node requires 820 $TALOS stake.

**2. Payment Medium (Optional)**
Users can pay in USDC or $TALOS. Paying in $TALOS provides a 15% discount, creating organic demand without forcing adoption.

### 5.2 Payment Flow

| Actor | Token Requirement |
|-------|-------------------|
| Workers | Must stake $TALOS |
| Users | Can pay in USDC or $TALOS |
| Settlement | Workers pay SOL gas fees |

Users never need to hold SOL. Workers absorb gas costs (profitable given job revenue).

### 5.3 Token Supply

**Max Supply:** 1,000,000,000 $TALOS (1 billion, fixed cap)

**Initial Distribution:**

| Allocation | Amount | Vesting |
|------------|--------|---------|
| Community & Ecosystem | 40% (400M) | 4-year linear unlock |
| Team & Advisors | 20% (200M) | 1-year cliff, 3-year linear |
| Investors | 15% (150M) | 6-month cliff, 2-year linear |
| Treasury | 15% (150M) | DAO-controlled, no vesting |
| Liquidity & Exchanges | 10% (100M) | Immediate |

```
┌─────────────────────────────────────────────────────────┐
│                    1B $TALOS                            │
├──────────────────────┬──────────────────────────────────┤
│   Community (40%)    │████████████████████              │
├──────────────────────┼──────────────────────────────────┤
│   Team (20%)         │██████████                        │
├──────────────────────┼──────────────────────────────────┤
│   Investors (15%)    │███████▌                          │
├──────────────────────┼──────────────────────────────────┤
│   Treasury (15%)     │███████▌                          │
├──────────────────────┼──────────────────────────────────┤
│   Liquidity (10%)    │█████                             │
└──────────────────────┴──────────────────────────────────┘
```

### 5.4 Emission Schedule

New tokens enter circulation through **Worker Rewards** — incentivizing early network participation before organic demand develops.

**Annual Emission (decreasing):**

| Year | Emission Rate | Tokens Released | Cumulative |
|------|---------------|-----------------|------------|
| 1 | 8% of max | 80M | 80M |
| 2 | 6% of max | 60M | 140M |
| 3 | 4% of max | 40M | 180M |
| 4 | 2% of max | 20M | 200M |
| 5+ | 1% of max | 10M/year | Capped at 300M total emissions |

**Total emission cap:** 300M $TALOS (30% of max supply)

After year 5, emissions continue at 1% until the 300M cap is reached (~Year 12), then emissions stop entirely. Network sustainability relies on fee revenue.

### 5.5 Staking Economics

Workers stake $TALOS to participate. Staking yield comes from two sources:

**Source 1: Protocol Emissions (decreasing over time)**
- Distributed pro-rata to staked workers
- Weighted by compute capacity provided
- Decreases annually per emission schedule

**Source 2: Fee Revenue (increasing with usage)**
- 10% of all job fees go to staking pool
- Distributed pro-rata to active workers
- Grows as network usage increases

```
           Staking Yield Composition Over Time

100% ┤
     │ ████████
     │ ████████████
     │ ████████████████   Emissions
     │ ████████████████████
 50% ┤ ████████████████████████
     │         ░░░░░░░░░░░░░░░░░░░░░░
     │              ░░░░░░░░░░░░░░░░░░░░░░░░
     │                   ░░░░░░░░░░░░░░░░░░░░░░░░░░  Fee Revenue
     │                        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░
  0% ┼────────────────────────────────────────────────────▶
     Year 1    Year 2    Year 3    Year 4    Year 5+
```

**Projected APY (depends on total staked and network usage):**

| Scenario | Total Staked | Network Revenue | Estimated APY |
|----------|--------------|-----------------|---------------|
| Early (Year 1) | 50M $TALOS | $1M/year | 15-25% |
| Growth (Year 2-3) | 150M $TALOS | $10M/year | 10-15% |
| Mature (Year 5+) | 300M $TALOS | $50M/year | 8-12% |

*APY varies based on stake participation and network revenue.*

### 5.6 Fee Structure

**Job Fees:**

| Payment Method | Protocol Fee | Worker Receives |
|----------------|--------------|-----------------|
| USDC | 5% | 95% |
| $TALOS | 2% | 98% (15% effective discount) |

**Fee Distribution:**

```
Job Fee (100%)
    │
    ├── 90% → Worker (direct payment)
    │
    └── 10% → Protocol
              │
              ├── 50% → Staking Pool (rewards)
              │
              ├── 30% → Treasury (development)
              │
              └── 20% → Burn (deflationary)
```

### 5.7 Token Sinks (Deflationary Pressure)

Multiple mechanisms reduce circulating supply:

**1. Fee Burns**
- 20% of protocol fees burned permanently
- At $50M annual revenue: ~$1M worth of $TALOS burned/year

**2. Slashing Burns**
- 50% of slashed stake is burned (rest goes to affected users)
- Penalizes bad actors while reducing supply

**3. Staking Lock-up**
- Staked tokens are illiquid
- 14-day unbonding period
- Target: 30-50% of supply staked

**Deflationary Crossover:**

At sufficient network revenue, burns exceed emissions:

```
Break-even calculation:
- Year 5 emissions: 10M $TALOS
- Required burns to offset: 10M $TALOS
- At 20% burn rate: need $50M protocol fees
- At 10% protocol take: need $500M job volume

Network becomes net-deflationary at ~$500M annual job volume.
```

### 5.8 Token Flow Diagram

```
                              ┌─────────────┐
                              │   USERS     │
                              └──────┬──────┘
                                     │
                         ┌───────────┴───────────┐
                         │                       │
                    Pay USDC                Pay $TALOS
                    (5% fee)                (2% fee)
                         │                       │
                         ▼                       ▼
              ┌─────────────────────────────────────────────┐
              │              PAYMENT CHANNELS                │
              │         (Solana PDAs / Escrow)              │
              └─────────────────────┬───────────────────────┘
                                    │
                         ┌──────────┴──────────┐
                         │                     │
                    90% to Worker         10% Protocol Fee
                         │                     │
                         ▼                     ▼
                  ┌──────────┐     ┌─────────────────────┐
                  │ WORKERS  │     │    PROTOCOL FEE     │
                  └────┬─────┘     └──────────┬──────────┘
                       │                      │
                  Stake $TALOS        ┌───────┼───────┐
                       │              │       │       │
                       ▼              ▼       ▼       ▼
              ┌──────────────┐    Stakers  Treasury  Burn
              │ STAKING POOL │     (50%)    (30%)   (20%)
              │              │◀──────┘
              │  Emissions + │
              │  Fee Share   │
              └──────────────┘
```

### 5.9 Economic Scenarios

**Bear Case (Low Adoption):**
- Year 3 job volume: $10M
- Protocol revenue: $1M
- Staking yield: ~5% (mostly emissions)
- Risk: Emissions dilute holders, price pressure

**Base Case (Moderate Growth):**
- Year 3 job volume: $100M
- Protocol revenue: $10M
- Staking yield: ~12%
- Outcome: Sustainable economics, growing ecosystem

**Bull Case (High Adoption):**
- Year 3 job volume: $500M
- Protocol revenue: $50M
- Staking yield: ~18%
- Outcome: Net deflationary, strong token demand

### 5.10 Worker Economics Example

**Setup:**
- Worker stakes 1,000 $TALOS (~$1,000 at $1/token)
- Provides 8 vCPU, 32GB RAM
- 50% utilization rate

**Monthly Revenue:**

| Source | Calculation | Amount |
|--------|-------------|--------|
| Job Revenue | 360 hrs × 50% util × $0.10/hr × 8 vCPU | $144 |
| Staking Yield | 1,000 × 12% APY / 12 months | $10 |
| **Total** | | **$154/month** |

**Annual ROI on stake:** ~185% (job revenue) + 12% (staking) = **~197%**

*Workers are incentivized to provide reliable service to maximize job allocation.*

---

## 6. Pricing

### 6.1 Worker-Set Pricing

Workers advertise their rates via gossip:

```json
{
  "pricing": {
    "cpu_ms": 0.000001,
    "memory_mb_ms": 0.0000001,
    "egress_mb": 0.01
  }
}
```

### 6.2 Job Cost Calculation

**Maximum cost** (locked when job starts):
```
max_cost = (vcpu × max_duration × cpu_rate) +
           (memory × max_duration × memory_rate)
```

**Actual cost** (charged after completion):
```
actual_cost = (vcpu × actual_duration × cpu_rate) +
              (memory × actual_duration × memory_rate) +
              (egress_bytes × egress_rate)
```

Unused balance remains in channel for subsequent jobs.

### 6.3 Job Tiers

| Tier | Max Duration | Max vCPU | Max Memory | Max Result |
|------|--------------|----------|------------|------------|
| Standard | 5 min | 4 | 8 GB | 50 MB |
| Compute | 30 min | 16 | 64 GB | 500 MB |

Workers advertise supported tiers. Compute tier requires higher stake.

---

## 7. Security

### 7.1 Triple-Layer Isolation

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during Docker `RUN` |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

### 7.2 Slashing Conditions

Workers are slashed only for **observable misbehavior**:

| Violation | Penalty |
|-----------|---------|
| No response to accepted job | 1% of stake |
| Abandonment (no result after timeout) | Job value + 1% stake |
| Repeated availability lies | Progressive slashing |

**Not slashable** (without TEE):
- Incorrect results (handled by reputation)
- Data exfiltration (mitigated by network allowlist)

### 7.3 Unbonding Period

Workers requesting stake withdrawal enter a 14-day unbonding period. This prevents "slash and run" attacks and allows time for fraud proofs.

### 7.4 Future: Confidential Compute

TEE integration (Intel SGX / AMD SEV) planned as premium tier for:
- Proprietary AI model inference
- Sensitive data processing
- Cryptographic proof of execution

---

## 8. Worker Selection

### 8.1 Geographic Routing

Workers announce regions in gossip:

```json
{
  "regions": ["us-west", "us-east"],
  "coordinates": [37.77, -122.41]
}
```

Users specify routing preferences:

```json
{
  "routing": {
    "required_regions": ["eu-*"],
    "preferred_regions": ["eu-west"],
    "max_latency_ms": 50
  }
}
```

**Matching Algorithm:**
1. Filter by `required_regions` (compliance)
2. Sort by preference match, price, reputation
3. If `max_latency_ms` set, probe top candidates

### 8.2 Reputation System

Workers build reputation based on:
- Job success rate
- Response latency (p50, p99)
- Uptime percentage
- Settlement history

High-reputation workers receive priority in job matching.

---

## 9. Failure Handling

### 9.1 Exit Codes

| Exit Code | Meaning | User Refund | Worker Paid |
|-----------|---------|-------------|-------------|
| 0 | Success | N/A | Yes |
| 1-127 | User code error | 0% | Yes |
| 128 | User timeout exceeded | 0% | Yes |
| 200 | Worker crash | 100% | No |
| 201 | Worker resource exhausted | 100% | No |
| 202 | Build failure | 50% | Partial |

### 9.2 Result Delivery

Results are stored as Iroh blobs with 24-hour TTL:
- User offline? Fetch later by hash
- Large results? Chunked streaming
- Need webhook? Optional URL in manifest

---

## 10. Job Orchestration

Talos supports composing multiple jobs into workflows, enabling pipelines, fan-out parallelism, and conditional execution.

### 10.1 Orchestration Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| **Single** | One job, no dependencies | Simple functions |
| **DAG** | Pre-declared dependency graph | Known pipelines |
| **Dynamic** | Jobs spawn children at runtime | Conditional logic |

### 10.2 DAG Mode (Static Workflows)

When the workflow structure is known upfront, declare it in the manifest:

```json
{
  "orchestration": {
    "mode": "dag",
    "jobs": {
      "fetch": {
        "code_hash": "blake3:aaa...",
        "deps_hash": "blake3:bbb...",
        "entrypoint": "fetch.py"
      },
      "process": {
        "code_hash": "blake3:ccc...",
        "deps_hash": "blake3:ddd...",
        "entrypoint": "process.py",
        "depends_on": ["fetch"],
        "input_from": "fetch"
      },
      "summarize": {
        "code_hash": "blake3:eee...",
        "deps_hash": "blake3:fff...",
        "entrypoint": "summarize.py",
        "depends_on": ["process"],
        "input_from": "process"
      }
    },
    "entry": "fetch",
    "affinity": "same_worker"
  }
}
```

**Benefits of DAG mode:**
- Worker can pre-warm downstream jobs (fetch dependencies while job N runs)
- Payment calculated upfront (max cost known before execution)
- Clear failure semantics (abort workflow or retry failed step)
- Cache sharing between jobs on same worker

**DAG Pipelining:**

```
Time 0:  [fetch: running]     [process: loading deps]   [summarize: queued]
Time 1:  [fetch: done] ────▶  [process: starts]         [summarize: loading deps]
Time 2:                       [process: done] ────────▶ [summarize: starts]
Time 3:                                                 [summarize: done]
```

The worker pipelines dependency loading with execution, minimizing total latency.

### 10.3 Dynamic Mode (Runtime Spawning)

When workflow shape depends on runtime decisions, jobs can spawn children programmatically:

```python
# Inside a Talos job
from talos import spawn, fan_out

# Sequential spawn
result = spawn(
    code="process.py",
    input=my_output,
    affinity="same_worker"
)

# Parallel fan-out
results = fan_out(
    code="analyze.py",
    inputs=[chunk1, chunk2, chunk3, chunk4],
    affinity="distributed"
)
```

**Spawn Limits (prevent runaway costs):**

```json
{
  "orchestration": {
    "mode": "dynamic",
    "spawn_limits": {
      "max_depth": 3,
      "max_total_jobs": 50,
      "max_spawn_budget_usdc": 10.0
    }
  }
}
```

| Limit | Default | Description |
|-------|---------|-------------|
| `max_depth` | 3 | Maximum nesting level (job → child → grandchild) |
| `max_total_jobs` | 50 | Maximum jobs spawned in entire workflow |
| `max_spawn_budget_usdc` | 10.0 | Budget cap for all spawned jobs |

If any limit is exceeded, spawn fails and parent job receives an error.

### 10.4 Affinity Controls

Control where child jobs execute:

| Affinity | Behavior | Best For |
|----------|----------|----------|
| `same_worker` | Run on same node | Sequential chains, shared cache |
| `same_region` | Run on nearby node | Compliance, moderate latency |
| `distributed` | Run on any available node | Fan-out parallelism |

**Same-worker benefits:**
- Zero network transfer for intermediate results
- Shared dependency cache (L2 hits)
- Sub-millisecond dispatch latency

**Distributed benefits:**
- Parallel execution across many nodes
- Not limited by single node's resources
- Better for CPU-bound fan-out

### 10.5 Inter-Job Data Passing

**Same-worker:** Results passed via shared memory or local filesystem. Zero serialization overhead for large artifacts.

**Distributed:** Results uploaded as Iroh blobs. Child job fetches by hash.

```
Same-worker:     Job A ──[memory]──▶ Job B     (< 1ms)
Distributed:     Job A ──[iroh blob]──▶ Job B  (network latency)
```

### 10.6 Payment for Child Jobs

**Same-worker (shared channel):**
- All jobs deduct from parent's payment channel
- Single settlement at workflow completion
- Worker tracks cumulative usage

**Distributed (nested tickets):**
- Parent issues sub-ticket for remote child
- Child's worker validates sub-ticket independently
- Each worker settles their portion

```
┌─────────────────────────────────────────────────────────────┐
│                     USER PAYMENT CHANNEL                     │
│                        Balance: 100 USDC                     │
└───────────────┬─────────────────────────────┬───────────────┘
                │                             │
         Ticket: 5 USDC                Sub-ticket: 3 USDC
                │                             │
                ▼                             ▼
         ┌──────────┐                  ┌──────────┐
         │ Worker A │ ───[spawn]────▶  │ Worker B │
         │ (parent) │                  │ (child)  │
         └──────────┘                  └──────────┘
```

### 10.7 Failure Handling in Workflows

| Failure | DAG Mode | Dynamic Mode |
|---------|----------|--------------|
| Job fails (user error) | Abort workflow, return partial results | Parent receives error, decides |
| Job fails (worker fault) | Retry on same/different worker | Parent can retry spawn |
| Spawn limit exceeded | N/A | Spawn returns error |
| Budget exhausted | Abort remaining jobs | Spawn returns error |

**Partial results:** For fan-out patterns, completed results are returned even if some branches fail. User code handles partial success.

### 10.8 Example: Map-Reduce Pattern

```json
{
  "orchestration": {
    "mode": "dag",
    "jobs": {
      "split": {
        "code_hash": "blake3:split...",
        "entrypoint": "split.py"
      },
      "map_0": { "depends_on": ["split"], "input_from": "split.chunks[0]", "..." : "..." },
      "map_1": { "depends_on": ["split"], "input_from": "split.chunks[1]", "..." : "..." },
      "map_2": { "depends_on": ["split"], "input_from": "split.chunks[2]", "..." : "..." },
      "map_3": { "depends_on": ["split"], "input_from": "split.chunks[3]", "..." : "..." },
      "reduce": {
        "depends_on": ["map_0", "map_1", "map_2", "map_3"],
        "input_from": ["map_0", "map_1", "map_2", "map_3"],
        "code_hash": "blake3:reduce...",
        "entrypoint": "reduce.py"
      }
    },
    "entry": "split",
    "affinity": {
      "split": "any",
      "map_*": "distributed",
      "reduce": "any"
    }
  }
}
```

```
              ┌─────────┐
              │  split  │
              └────┬────┘
       ┌──────┬───┴────┬──────┐
       ▼      ▼        ▼      ▼
   ┌──────┐┌──────┐┌──────┐┌──────┐
   │map_0 ││map_1 ││map_2 ││map_3 │  (parallel, distributed)
   └──┬───┘└──┬───┘└──┬───┘└──┬───┘
       └──────┴───┬────┴──────┘
                  ▼
              ┌────────┐
              │ reduce │
              └────────┘
```

---

## 11. Network Topology

### 11.1 Discovery

All workers subscribe to `talos-compute-v1` gossip topic. Announcements include:
- Node ID (Ed25519 public key)
- Capabilities (vCPU, RAM, GPU, regions)
- Pricing
- Current load

### 11.2 Direct Connections

After discovery, users connect directly to workers via Magicsock:
- NAT traversal via UDP hole-punching
- Fallback to DERP relays
- Connection identified by public key (not IP)

### 11.3 Global Cache

Dependency blobs are content-addressed and shared peer-to-peer:
- Node A builds `pytorch-v2` → announces hash
- Node B needs same deps → fetches from A (or any seeder)
- Popular dependencies propagate network-wide

### 11.4 Worker Lifecycle State Machine

```
                         ┌────────────────┐
                         │                │
        Install binary   │  UNREGISTERED  │
       ──────────────────▶                │
                         │                │
                         └───────┬────────┘
                                 │
                                 │ Stake $TALOS on Solana
                                 ▼
                         ┌────────────────┐
                         │                │
                         │   REGISTERED   │
                         │ (stake locked) │
                         │                │
                         └───────┬────────┘
                                 │
                                 │ Join gossip network
                                 ▼
                         ┌────────────────┐
                         │                │◀──────────────────┐
                         │    ONLINE      │                   │
                         │ (advertising)  │                   │
                         │                │                   │
                         └───────┬────────┘                   │
                                 │                            │
               ┌─────────────────┼─────────────────┐          │
               │                 │                 │          │
               ▼                 ▼                 ▼          │
       ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
       │              │  │              │  │              │   │
       │    BUSY      │  │   DRAINING   │  │   OFFLINE    │───┘
       │ (at capacity)│  │ (no new jobs)│  │ (connection  │
       │              │  │              │  │    lost)     │
       └──────┬───────┘  └──────┬───────┘  └──────────────┘
              │                 │
              │                 │ All jobs complete
              │                 ▼
              │          ┌──────────────┐
              │          │              │
              │          │  UNBONDING   │
              │          │  (14 days)   │
              │          │              │
              │          └──────┬───────┘
              │                 │
              │                 │ Unbonding period ends
              │                 ▼
              │          ┌──────────────┐
              │          │              │
              └─────────▶│   EXITED     │
                         │(stake returned│
                         │              │
                         └──────────────┘
```

**Worker States:**

| State | Can Accept Jobs | Stake Status |
|-------|-----------------|--------------|
| UNREGISTERED | No | None |
| REGISTERED | No | Locked |
| ONLINE | Yes | Locked |
| BUSY | No (at capacity) | Locked |
| DRAINING | No | Locked |
| OFFLINE | No | Locked (slashing risk) |
| UNBONDING | No | Locked (14 day wait) |
| EXITED | No | Returned |

**State Transitions:**

| From | To | Trigger |
|------|----|---------|
| UNREGISTERED | REGISTERED | `stake()` tx confirmed |
| REGISTERED | ONLINE | Join gossip, start heartbeat |
| ONLINE | BUSY | All job slots filled |
| BUSY | ONLINE | Job slot freed |
| ONLINE | DRAINING | Operator initiates shutdown |
| DRAINING | UNBONDING | All active jobs complete |
| ONLINE | OFFLINE | Heartbeat timeout (5 min) |
| OFFLINE | ONLINE | Reconnect within grace period |
| OFFLINE | SLASHED | Grace period exceeded (1 hr) |
| UNBONDING | EXITED | 14 days elapsed |

---

## 12. Roadmap

### Phase 1: Engine (Q1 2026)
- Single-node worker binary
- Iroh P2P networking
- Firecracker + Unikraft integration
- Mock payment verification

### Phase 2: Network (Q2 2026)
- Multi-node testnet
- Solana Devnet integration
- Payment channel implementation
- Global cache syncing

### Phase 3: Launch (Q3 2026)
- Anchor program audit
- $TALOS token generation
- Mainnet launch
- SDK release (Python, TypeScript, Rust)

### Phase 4: Scale (Q4 2026+)
- GPU compute support
- Confidential compute tier (TEE)
- Geographic expansion
- Enterprise features

---

## 13. Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| Settlement | Solana + Anchor | Payment channels, staking |
| Networking | Iroh | P2P discovery, data transfer |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft + BuildKit | Dockerfile → minimal kernel |
| Signatures | Ed25519 | Payment tickets, identity |

---

## 14. Comparison

| Feature | AWS Lambda | Akash | Talos |
|---------|------------|-------|-------|
| Cold Start | 100-500ms | 30-120s | 200-500ms |
| Isolation | Container | Container | MicroVM |
| Payment | Credit Card | $AKT | USDC / $TALOS |
| Latency | Centralized | On-chain | Off-chain |
| Permissionless | No | Yes | Yes |

---

## Appendix A: Manifest Schema

```json
{
  "$schema": "https://talos.network/schemas/manifest-v1.json",
  "version": "1.0",
  "kernel": "python-3.11-unikraft",
  "entrypoint": "main.py",
  "resources": {
    "tier": "standard",
    "vcpu": 2,
    "memory_mb": 2048,
    "max_duration_ms": 60000
  },
  "network": {
    "egress_allowlist": ["api.openai.com", "*.amazonaws.com"]
  },
  "routing": {
    "required_regions": null,
    "preferred_regions": ["us-west"],
    "max_latency_ms": null
  },
  "result": {
    "max_size_mb": 50,
    "webhook_url": null
  },
  "assets": {
    "requirements_hash": "blake3:abc...",
    "requirements_url": "iroh:blob/abc...",
    "code_hash": "blake3:def...",
    "code_url": "iroh:blob/def...",
    "input_hash": "blake3:ghi...",
    "input_url": "iroh:blob/ghi..."
  }
}
```

## Appendix B: Payment Ticket Schema

```json
{
  "channel_id": "base58-encoded-pda",
  "amount_micros": 50000,
  "nonce": 42,
  "timestamp": 1706900000,
  "signature": "base64-ed25519-signature"
}
```

## Appendix C: Orchestration Schema

```json
{
  "orchestration": {
    "mode": "dag | dynamic | single",

    "dag": {
      "jobs": {
        "<job_id>": {
          "code_hash": "blake3:...",
          "deps_hash": "blake3:...",
          "entrypoint": "main.py",
          "resources": {
            "vcpu": 2,
            "memory_mb": 2048,
            "max_duration_ms": 60000
          },
          "depends_on": ["<parent_job_id>"],
          "input_from": "<parent_job_id> | <parent_job_id>.field"
        }
      },
      "entry": "<starting_job_id>",
      "affinity": {
        "default": "same_worker | same_region | distributed",
        "<job_id>": "same_worker | same_region | distributed"
      }
    },

    "spawn_limits": {
      "max_depth": 3,
      "max_total_jobs": 50,
      "max_spawn_budget_usdc": 10.0
    }
  }
}
```

## Appendix D: Worker Announcement Schema

```json
{
  "node_id": "ed25519:base58...",
  "version": "0.1.0",
  "capabilities": {
    "tiers": ["standard", "compute"],
    "max_vcpu": 16,
    "max_memory_mb": 65536,
    "gpu": null,
    "kernels": ["python-3.11-unikraft", "node-20-unikraft"]
  },
  "regions": ["us-west-2"],
  "coordinates": [45.52, -122.67],
  "pricing": {
    "cpu_ms_micros": 1,
    "memory_mb_ms_micros": 0.1,
    "egress_mb_micros": 10000
  },
  "stake": {
    "amount": 1000,
    "locked_until": null
  },
  "reputation": {
    "success_rate": 0.997,
    "jobs_completed": 50000,
    "p50_latency_ms": 180,
    "p99_latency_ms": 450
  }
}
```

---

*For technical questions: developers@talos.network*
*For partnerships: partners@talos.network*
