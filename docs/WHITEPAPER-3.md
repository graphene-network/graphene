This is the **Talos Network Technical Whitepaper (v3.0)**. It reflects our final architectural decision: a sovereign AppChain for settlement, pure P2P for data, and state channels for zero-latency payments.

---

# The Talos Network: A Zero-Latency, Decentralized JIT Cloud

**Version:** 3.0 (State Channel Architecture)
**Date:** February 2026

## 1. Abstract

Talos is a decentralized physical infrastructure network (DePIN) designed for high-performance, ephemeral cloud computing. Unlike first-generation decentralized clouds that suffer from blockchain latency (6s+ block times) and massive container download overheads, Talos introduces a **Just-in-Time (JIT) Hypervisor** paired with **Unidirectional Payment Channels**. This architecture allows for sub-second job execution and instant, trustless payments, enabling a user experience comparable to AWS Lambda but on a permissionless, sovereign network.

## 2. The Core Problems

1. **The Docker Bottleneck:** Shipping gigabyte-sized containers for every job is too slow for real-time tasks.
2. **The Blockchain Lag:** Waiting for block finality (consensus) before starting a job destroys latency-sensitive use cases (AI inference, web requests).
3. **The Fair Exchange Dilemma:** Workers fear non-payment; Users fear non-delivery.

## 3. The Talos Solution: "Signal Off-Chain, Settle On-Chain"

Talos decouples **Work** from **Settlement**.

* **Work** happens instantly over a P2P network (Iroh).
* **Payment** happens instantly via cryptographic tickets (State Channels).
* **Settlement** happens asynchronously on the blockchain (Substrate).

## 4. System Architecture (The "Talos Stack")

The entire stack is written in **Rust**, ensuring memory safety and performance from the kernel to the network layer.

### Layer 1: The Consensus Plane (Substrate AppChain)

**Role:** "The Bank" & "The Court"
A sovereign Layer-1 blockchain built on the **Polkadot SDK**.

* **Purpose:** It does *not* store job data or queues. It strictly manages **Identities** (Validator/Worker Registry) and **Escrow Vaults** (Payment Channels).
* **Logic:** The custom `pallet-talos` module handles the opening of channels ("The Tab") and the final settlement of funds based on signed tickets.

### Layer 2: The Data & Control Plane (Iroh)

**Role:** "The Courier"
A pure P2P network using **QUIC** and **Gossip** protocols.

* **Discovery:** Workers announce their capabilities via the `talos-global-compute` gossip topic.
* **Transport:** Data (code, inputs, results) is streamed directly between User and Worker via encrypted, NAT-traversing connections (Magicsock).
* **Global Hot Cache:** Dependency drives (e.g., `pytorch-v2.img`) are content-addressed blobs. Once one node downloads a library, it can seed it to peers, creating a "BitTorrent-like" global cache.

### Layer 3: The Execution Plane (JIT Firecracker)

**Role:** "The Factory"
A custom Rust hypervisor managing **Firecracker MicroVMs**.

* **The Sandwich Model:** VMs are assembled dynamically at runtime by layering three block devices:
1. **Kernel (Read-Only):** The immutable Linux kernel.
2. **Dependencies (Read-Only):** Shared, cached library drives (mounted instantly).
3. **Code (Read/Write):** The tiny, ephemeral user script.


* **Cold Start:** < 500ms.

### Layer 4: The Economic Plane (Schnorrkel State Channels)

**Role:** "The Cash Register"

* **Mechanism:** Unidirectional Payment Channels.
* **The Ticket:** A User sends a cryptographically signed off-chain message ("Ticket") to the Worker alongside the job payload.
* **Verification:** The Worker validates the signature locally (<1ms). If valid, execution starts immediately. No blockchain transaction is required per job.

---

## 5. The Workflow: Zero-Latency Lifecycle

The following process ensures the user experiences "Instant" cloud compute while the worker remains financially secure.

### Phase 1: The Setup (One-Time)

1. **Locking Funds:** The User sends a transaction to the **Talos Chain** to open a channel with the Worker (or a Gateway).
* *Action:* "Lock 100 TALOS in the vault."
* *Result:* The chain records `ChannelID: 55` with `Balance: 100`.



### Phase 2: The Job (Real-Time loop)

2. **Submission (P2P):** The User broadcasts a job via **Iroh**:
* Payload: `{ script_hash: "abc...", input: "data.json" }`
* Payment: `{ channel_id: 55, amount: 1.0, signature: "schnorr_sig_xyz" }`


3. **Verification (Local):**
* The Worker receives the message.
* Checks local cache: "Does Channel 55 exist?" **Yes.**
* Checks crypto: "Is this signature valid for 1.0 TALOS?" **Yes.**


4. **Execution (JIT):**
* The Worker boots the Firecracker VM.
* Latencies: Verification (1ms) + Boot (200ms) + Run (Time).


5. **Return:** The Worker streams the result back to the User via Iroh.

### Phase 3: Settlement (Asynchronous)

6. **Cashing Out:** After 1,000 jobs (or at the end of the day), the Worker submits the **Final Ticket** (e.g., "Total: 100 TALOS") to the blockchain.
7. **Finality:** The `pallet-talos` verifies the signature, closes the channel, and transfers 100 TALOS from the Vault to the Worker's wallet.

---

## 6. Security & Trust

### Why Users are Safe (The "Glass Box")

Funds are held in a smart contract, not the Worker's wallet.

* **Refunds:** If a Worker goes offline, the User waits for the `CheckLockTime` (e.g., 24 hours) and reclaims their funds via a Timeout transaction.
* **Control:** The Worker can never take more than the User has signed for.

### Why Workers are Safe (The "IOU")

* **Guaranteed Pay:** A valid Ticket is a mathematically proven claim on the on-chain vault. The blockchain *must* honor it.
* **Risk Limits:** Workers can set a "Max Unsettled Limit." If a User has 50 TALOS worth of pending tickets, the Worker can pause and settle before accepting more work.

---

## 7. Roadmap

### Phase 1: The Engine (Alpha)

* **Deliverable:** Single-node Rust binary.
* **Features:** Iroh P2P networking, Firecracker VMM integration, "Global Hot Cache" for dependencies.
* **Status:** *In Development.*

### Phase 2: The Market (Beta)

* **Deliverable:** Multi-node Testnet.
* **Features:** Integration of `schnorrkel` signature verification. Workers reject jobs without valid tickets. Mock payment channel logic (in-memory).

### Phase 3: The Chain (Mainnet)

* **Deliverable:** Talos AppChain (Substrate).
* **Features:** `pallet-talos` for handling deposits and settlements. Full economic integration. Public launch.
