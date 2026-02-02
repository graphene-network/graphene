Here is the **Official Technical Whitepaper for the Talos Network (v4.0)**. This document consolidates every architectural decision we have made, moving from a custom Layer-1 to the high-performance, asset-light Solana architecture.

---

# The Talos Network

**A Zero-Latency, JIT-Optimized Decentralized Cloud**

**Version:** 4.0 (Solana Asset-Light Architecture)
**Date:** February 2026

---

## 1. Abstract

Talos is a decentralized physical infrastructure network (DePIN) designed for high-performance, ephemeral cloud computing. It solves the "Cold Start" and "Blockchain Latency" problems inherent in current decentralized clouds.

By combining a **Just-in-Time (JIT) Hypervisor** (Firecracker) with a **Peer-to-Peer Data Plane** (Iroh) and utilizing **Solana** for high-speed settlement, Talos achieves sub-second job execution times comparable to centralized serverless platforms (AWS Lambda), all while maintaining a permissionless, trustless architecture.

## 2. The Core Problems

Current DePIN compute networks suffer from three structural bottlenecks:

1. **The Docker Bottleneck:** Shipping gigabyte-sized container images for every small job creates unacceptable latency and bandwidth congestion.
2. **The Consensus Lag:** Waiting for blockchain block finality (even 400ms) before starting a job destroys real-time use cases like AI inference or web requests.
3. **The Gas Friction:** Users are forced to hold native gas tokens (SOL/ETH) and sign transactions for every single job, ruining the user experience.

## 3. The Talos Solution: "Asset-Light" Architecture

Talos rejects the premise that a compute network needs its own blockchain. Instead, we leverage the most performant existing tools for each layer of the stack, focusing our engineering efforts strictly on the compute engine.

* **Settlement:** **Solana** (Anchor) – Chosen for high throughput and cheap verification of Ed25519 signatures.
* **Networking:** **Iroh** (Rust) – Chosen for QUIC-based streaming and NAT traversal.
* **Compute:** **Firecracker** (Rust) – Chosen for microsecond boot times and strong isolation.

## 4. Technical Architecture (The 4 Layers)

The entire node stack is written in **Rust**, ensuring memory safety and unified logic from the kernel to the blockchain client.

### Layer 1: The Settlement Plane (Solana + Anchor)

**Role:** "The Judge" & "The Bank"
Talos utilizes a custom Anchor Program on Solana to handle financial security without blocking execution.

* **SPL Tokens:** Payments are denominated in standard SPL tokens (USDC or $TALOS).
* **Instruction Introspection:** The smart contract uses Solana's native `Ed25519Program` to verify bulk payment tickets cheaply.
* **Fee Abstraction:** Users sign off-chain messages; Workers pay the SOL gas fees during settlement. The User never needs SOL.

### Layer 2: The Data Plane (Iroh P2P)

**Role:** "The Courier"
A pure P2P network that bypasses the blockchain entirely for data transfer.

* **Gossip Protocol:** Workers announce availability via the `talos-global-compute` topic.
* **Direct Tunnels:** Job payloads stream directly from User to Worker via encrypted QUIC tunnels, punching through home NATs (Magicsock).
* **Global Hot Cache:** Dependency drives (e.g., `pytorch-v2.img`) are content-addressed blobs. Once Node A downloads a library, it seeds it to Node B, creating a global, shared cache.

### Layer 3: The Execution Plane (JIT Firecracker)

**Role:** "The Factory"
Talos replaces Docker with a custom Rust hypervisor managing Firecracker MicroVMs.

* **The Sandwich Model:** VMs are assembled dynamically at runtime by layering three block devices:
1. **Kernel (Read-Only):** The immutable Linux kernel.
2. **Dependencies (Read-Only):** Shared, cached library drives (mounted instantly).
3. **Code (Read/Write):** The tiny, ephemeral user script.


* **Performance:** < 500ms Cold Start.

### Layer 4: The Economic Plane (State Channels)

**Role:** "The Cash Register"

* **Unidirectional Payment Channels:** Users lock funds *once* on-chain.
* **The Ticket:** For every job, the User sends a cryptographically signed "Ticket" (off-chain) to the Worker.
* **Verification:** The Worker validates the signature locally (<1ms). If valid, execution starts immediately.

---

## 5. The Workflow: Zero-Latency Lifecycle

### Phase 1: Setup (The "Tab")

1. **Open Channel:** The User sends a transaction to the Solana Anchor Program.
* *Action:* "Lock 100 USDC in a PDA for Worker X."
* *Result:* On-chain state records the deposit.



### Phase 2: Execution (The Loop)

2. **Submission (P2P):** The User broadcasts a job via Iroh.
* *Payload:* `{ script_hash: "abc...", input_data: "..." }`
* *Payment:* `{ channel: "UserPubkey", amount: 1.0, nonce: 5, signature: "Ed25519_Sig" }`


3. **Verification (Local):**
* Worker verifies the Ed25519 signature against the User's Pubkey.
* Worker checks the on-chain account to ensure the balance > 1.0 USDC.
* *Time:* < 5ms.


4. **JIT Boot:**
* Worker pulls missing dependencies from the Iroh Swarm.
* Worker boots Firecracker VM and runs the script.


5. **Return:** Result streams back to User via Iroh.

### Phase 3: Settlement (Async)

6. **Cash Out:** After 1,000 jobs, the Worker submits the **Final Ticket** to Solana.
7. **Introspection:** The Anchor Program checks the Ed25519 signature validity and transfers the total amount (e.g., 1000 USDC) from the Vault to the Worker.

---

## 6. Tokenomics

The **$TALOS** token (SPL) serves two primary functions:

1. **Staking (Security):** Workers must stake $TALOS to join the network. Malicious behavior (proven via fraud proofs) results in slashing.
2. **Compute Units (Medium of Exchange):** While USDC is supported, paying in $TALOS offers fee discounts.

## 7. Roadmap

* **Phase 1: The Engine (Alpha)**
* Single-node Rust binary (`talos-worker`).
* Iroh P2P networking + Firecracker integration.
* Mock payment verification.


* **Phase 2: The Network (Testnet)**
* Deploy Anchor Program to Solana Devnet.
* Implement Ed25519 verification in Rust Worker.
* "Global Hot Cache" syncing between nodes.


* **Phase 3: The Launch (Mainnet)**
* Audited Anchor contracts.
* SPL Token generation.
* Public mainnet launch.
