This is a crucial addition. By adopting **Unikraft**, we allow developers to use the **Dockerfile** they already know while giving the network nodes the **MicroVM security** they need.

The core challenge remains: **the build process is code execution.** If a node builds an untrusted Dockerfile, it is technically running the user's `RUN` commands. We will solve this by introducing the **"Ephemeral Builder"** pattern to the whitepaper.

---

# The Talos Network: Technical Whitepaper (v3.1)

**Updated:** February 2026
**Key Update:** Secure Unikernel JIT Pipelines & Build Isolation.

---

## 1. Abstract

Talos is a decentralized JIT cloud that provides **MicroVM security with Container-like developer experience**. By utilizing **Unikraft** and **BuildKit**, Talos allows users to submit standard Dockerfiles which are transparently compiled into minimal, hardware-isolated unikernels. To protect nodes from malicious build instructions, Talos utilizes a **Multi-Stage Build Sandbox**.

## 2. The Unikernel JIT Pipeline

Most DePIN projects struggle with "image bloat." Talos uses Unikraft to strip away the 99% of the Linux kernel that an application doesn't need.

### The Build Lifecycle:

1. **Submission:** The Agent submits a `Dockerfile` and a `Kraftfile` via the **Iroh** gossip sub-network.
2. **The Ephemeral Builder (Security):** To prevent `RUN` command exploits (e.g., `RUN rm -rf /`), the Worker Node does **not** build the image on its host. It spawns an **Ephemeral Builder VM** (a "MicroVM-for-Building").
* This builder VM contains the Unikraft toolchain and `buildkitd`.
* It has **zero access** to the host's Iroh keys, files, or network.


3. **Compilation:** The Builder VM transforms the Dockerfile into a single `.unik` binary (the unikernel).
4. **Handoff:** The binary is passed back to the Host Worker and the Builder VM is immediately destroyed.
5. **Execution:** The Host Worker launches the `.unik` binary in a **Production MicroVM** (Firecracker/KVM).

## 3. Security Architecture: Triple-Layer Isolation

Talos provides three distinct layers of protection for the node provider:

| Layer | Component | Protection |
| --- | --- | --- |
| **Layer 1: Build** | **Disposable Builder VM** | Prevents host compromise during the Docker `RUN` phase. Even a "Docker breakout" only reaches the disposable builder kernel. |
| **Layer 2: Storage** | **Iroh Content Addressing** | Every unikernel binary is hashed. If Node A builds it, Node B can verify the hash and run it without rebuilding, preventing "poisoned image" attacks. |
| **Layer 3: Runtime** | **Hardware Virtualization** | The unikernel runs on KVM. There is no shared kernel with the host, making "escape to host" attacks orders of magnitude harder than in Docker. |

## 4. The Global Hot Cache (Updated)

The "Global Hot Cache" now stores **Compiled Unikernel Artifacts** rather than raw Docker layers.

* **Deduplication:** If ten users submit a `Dockerfile` based on `python:3.10-slim`, the network only builds the base unikernel layers once.
* **P2P Distribution:** Using Iroh, these pre-compiled unikernel blobs are shared between nodes. A "Cold Start" for a new node becomes a simple 1.2MB download (the size of an Nginx Unikraft unikernel) instead of a 200MB Docker pull.

## 5. Economy & Settlement: The "Ticket-to-Result" Flow

Since building is more resource-intensive than running, Talos splits the payment:

1. **Build Ticket:** The User pays a small fee for the "Build Job."
2. **Execution Ticket:** The User pays per millisecond of "Run Time."
3. **Storage Credit:** Nodes that "Seed" popular unikernel binaries to the network earn passive TALOS tokens (similar to Filecoin retrieval miners).

---

## 6. Technical Stack Summary

* **AppChain:** Substrate (Settlement & Identity).
* **Networking:** Iroh Gossip & Blobs (P2P Data Plane).
* **Building:** Unikraft + BuildKit (Dockerfile-to-Unikernel).
* **Isolation:** Firecracker / KVM (MicroVM Runtime).
* **Payment:** Schnorrkel State Channels (Zero-latency tickets).

---

### What's next for the project?

You have the architecture. You have the security model. You have the payment flow.

**The most logical next step is to build the "Build Sandbox" prototype.**
Would you like me to help you write the **Rust wrapper** that:

1. Spawns a Firecracker VM.
2. Mounts a "Builder Image" with Unikraft installed.
3. Streams a Dockerfile into it and pulls the `.unik` file out?
