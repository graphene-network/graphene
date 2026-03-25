# Graphene

**Secure code execution for AI agents.**

> Graphene is in active development. An alpha release will be available for testing soon.

Graphene is an open-source runtime that lets AI agents execute code inside hardware-isolated MicroVMs. Each job runs in a sealed unikernel with no shell, no package manager, and no arbitrary network access — eliminating the attack surface that makes container-based agent execution dangerous.

## The Problem

AI agent platforms give agents shell access inside containers. This is fundamentally broken:

- AI hallucinations can run `rm -rf /` or `curl malware.com | bash`
- Prompt injection attacks trick agents into executing exploits
- Compromised packages in the supply chain affect the entire system
- Unrestricted network egress enables data exfiltration

**The shell is the wrong abstraction for AI agents.** They need to execute code, not operate environments.

## How Graphene Works

Submit code via HTTP. Graphene compiles it into a sealed unikernel and runs it in a Firecracker MicroVM:

```
  AI Agent (your code)
      │
      │  POST /v1/jobs  { code, runtime, resources }
      ▼
  ┌─────────────────────────────────────┐
  │  Graphene Worker                    │
  │                                     │
  │  1. Encrypt code (XChaCha20)        │
  │  2. Build unikernel (if not cached) │
  │  3. Boot MicroVM (<200ms)           │
  │  4. Execute, return result          │
  └─────────────────────────────────────┘
      │
      ▼
  Encrypted result  { output, exitCode, metrics }
```

The unikernel has:
- **No shell** — `/bin/bash` doesn't exist
- **No package manager** — all deps baked in at build time
- **No process spawning** — single-process architecture
- **No arbitrary network access** — egress allowlist only

## Quick Start

```typescript
import { Client } from '@graphene/sdk';

const client = await Client.create({
  secretKey: mySecretKey,
  channelId: channelId,
  workerPubkey: workerPubkey,
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

console.log(new TextDecoder().decode(result.output)); // {"sum": 15}
```

With egress allowlists and Python:

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
```

## Comparison

| Feature | Cloudflare Workers | Northflank Sandboxes | AWS Lambda | Graphene |
|---------|-------------------|---------------------|------------|----------|
| Cold Start | <1ms | <1s | 100-500ms | **200-500ms** |
| Isolation | V8 Isolate | MicroVM (Kata/gVisor) | Container | **MicroVM + Unikernel** |
| Runtimes | JS/WASM only | Any language | Many (containers) | **Python, Node, Bun** |
| Shell Access | No | Yes | Yes (risky) | **No (by design)** |
| Network Egress | Unrestricted | Configurable | Unrestricted | **Allowlist only** |
| Arbitrary Binaries | No | Yes | Yes | **Yes (build-time)** |
| E2E Encryption | No | No | No | **Yes (per-job keys)** |
| Self-Hostable | No | Yes (BYOC) | No | **Yes** |
| Open Source | No | No | No | **Yes (AGPL-3.0)** |

### vs Cloudflare Workers

Cloudflare Workers run inside V8 isolates — a software sandbox sharing a process with other tenants.

- **JS/WASM only**: No Python, no native binaries, no `ffmpeg`, no ML frameworks. Graphene runs full unikernels with any statically-linked binary.
- **Software isolation**: A V8 bug = tenant escape. Graphene uses KVM hardware virtualization — each job gets its own virtual machine.
- **Centralized**: You can't run Workers on your own hardware. Graphene workers run anywhere with KVM support.

### vs Northflank Sandboxes

Northflank uses Kata/gVisor MicroVMs — similar isolation to Graphene, but with a full Linux userland inside.

- **Shell access**: Northflank sandboxes have shells. Graphene unikernels don't — the attack surface is fundamentally smaller.
- **Unikernel images**: 1-5MB vs container images. Sub-second cold starts from content-addressed cache.
- **E2E encryption**: Graphene encrypts job I/O with per-job ephemeral keys. Code and results are encrypted in transit and at rest.

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

## Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| API | Axum (HTTP REST) | Job submission, management |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft | Dockerfile → minimal kernel |
| Encryption | XChaCha20-Poly1305 | Per-job E2E encryption |
| Signatures | Ed25519 | Identity, key derivation |
| Hashing | BLAKE3 | Content addressing |

## Security Model

Triple-layer isolation:

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during `RUN` commands |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

## Documentation

- [Whitepaper](docs/WHITEPAPER.md) — Full technical specification
- [Development Guide](docs/DEVELOPMENT.md) — Running tests locally

## License

GNU Affero General Public License v3.0

---

*For technical questions: developers@graphene.network*
*For partnerships: partners@graphene.network*
