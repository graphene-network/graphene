# Development Guide

## Running E2E Tests

The E2E tests require Firecracker, which depends on KVM (Linux only). This guide covers running tests on different platforms.

### Prerequisites

The E2E tests need:
- **Firecracker** - MicroVM hypervisor (requires KVM)
- **Solana CLI** - For program deployment
- **Anchor CLI** - For building the Solana program
- **Kraft CLI** - For building unikernel images
- **Bun** - JavaScript runtime

### Linux (Native)

On Linux with KVM support:

```bash
# Install system dependencies (including OpenSSL for Rust builds)
sudo apt-get update
sudo apt-get install -y \
    e2fsprogs \
    build-essential \
    pkg-config \
    libssl-dev \
    curl

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Install Bun
curl -fsSL https://bun.sh/install | bash
source ~/.bashrc

# Install Firecracker
curl -fsSL "https://github.com/firecracker-microvm/firecracker/releases/download/v1.11.0/firecracker-v1.11.0-x86_64.tgz" -o firecracker.tgz
tar -xzf firecracker.tgz
sudo mv release-v1.11.0-x86_64/firecracker-v1.11.0-x86_64 /usr/local/bin/firecracker
sudo chmod +x /usr/local/bin/firecracker

# Install Kraft CLI (for building unikernels)
curl --proto '=https' --tlsv1.2 -sSf https://get.kraftkit.sh | sh

# Install Solana CLI
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
echo 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Generate keypair for program deployment
solana-keygen new --no-bip39-passphrase

# Install Anchor CLI
cargo install --git https://github.com/coral-xyz/anchor --tag v0.32.1 anchor-cli
```

#### Build Kernels

The E2E tests require pre-built unikernel images in the cache:

```bash
# Create kernel cache directory
mkdir -p ~/.graphene/cache/kernels

# Build Python 3.12 kernel (required for most tests)
cd kernels/python/3.12
kraft build --plat fc --arch x86_64
kraft pkg pull -w .unikraft/pkg --plat fc --arch x86_64 "unikraft.org/python:3.12"
cp .unikraft/pkg/unikraft/bin/kernel ~/.graphene/cache/kernels/python-3.12_fc-x86_64

# Build other kernels as needed
cd ../3.10
kraft build --plat fc --arch x86_64
kraft pkg pull -w .unikraft/pkg --plat fc --arch x86_64 "unikraft.org/python:3.10"
cp .unikraft/pkg/unikraft/bin/kernel ~/.graphene/cache/kernels/python-3.10_fc-x86_64

cd ../../node/21
kraft build --plat fc --arch x86_64
kraft pkg pull -w .unikraft/pkg --plat fc --arch x86_64 "unikraft.org/node:21"
cp .unikraft/pkg/unikraft/bin/kernel ~/.graphene/cache/kernels/node-21_fc-x86_64
```

#### Build and Run Tests

```bash
# Build the worker binary
cd crates/node
cargo build --bin graphene-worker --release

# Build the Anchor program
cd ../../programs/graphene
anchor build

# Build and test the SDK
cd ../../sdks/node
bun install
bun run build
bun test ./tests/*.e2e.test.ts
```

### macOS (via Lima)

macOS doesn't support KVM natively. Use [Lima](https://lima-vm.io/) to run a Linux VM with nested virtualization.

#### 1. Install Lima

```bash
brew install lima
```

#### 2. Create VM with Nested Virtualization

```bash
limactl start --set '.nestedVirtualization=true' --name=graphene template://ubuntu
```

#### 3. Shell into the VM

```bash
limactl shell graphene
```

#### 4. Install Dependencies (inside VM)

```bash
# System packages
sudo apt-get update
sudo apt-get install -y e2fsprogs curl build-essential pkg-config libssl-dev

# Firecracker
curl -fsSL "https://github.com/firecracker-microvm/firecracker/releases/download/v1.11.0/firecracker-v1.11.0-x86_64.tgz" -o firecracker.tgz
tar -xzf firecracker.tgz
sudo mv release-v1.11.0-x86_64/firecracker-v1.11.0-x86_64 /usr/local/bin/firecracker
sudo chmod +x /usr/local/bin/firecracker

# Verify KVM access
ls -la /dev/kvm

# Kraft CLI (for building unikernels)
curl --proto '=https' --tlsv1.2 -sSf https://get.kraftkit.sh | sh

# Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Solana CLI
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
echo 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Generate keypair
solana-keygen new --no-bip39-passphrase

# Anchor CLI
cargo install --git https://github.com/coral-xyz/anchor --tag v0.32.1 anchor-cli

# Bun
curl -fsSL https://bun.sh/install | bash
source ~/.bashrc
```

#### 5. Navigate to Project

Lima mounts your home directory by default:

```bash
cd /Users/$(whoami)/Git/graphene
```

#### 6. Build Kernels

```bash
# Create kernel cache directory
mkdir -p ~/.graphene/cache/kernels

# Build Python 3.12 kernel
cd kernels/python/3.12
kraft build --plat fc --arch x86_64
kraft pkg pull -w .unikraft/pkg --plat fc --arch x86_64 "unikraft.org/python:3.12"
cp .unikraft/pkg/unikraft/bin/kernel ~/.graphene/cache/kernels/python-3.12_fc-x86_64

# Return to project root
cd /Users/$(whoami)/Git/graphene
```

#### 7. Build and Run Tests

```bash
# Build the worker binary
cd crates/node
cargo build --bin graphene-worker --release

# Build the Anchor program
cd ../../programs/graphene
anchor build

# Build and test the SDK
cd ../../sdks/node
bun install
bun run build
export GRAPHENE_KERNEL_CACHE="$HOME/.graphene/cache/kernels"
bun test ./tests/*.e2e.test.ts
```

#### Stopping the VM

```bash
# From macOS host
limactl stop graphene

# To delete the VM entirely
limactl delete graphene
```

---

## CI/CD Workflows

The project uses GitHub Actions with multiple workflows to ensure code quality and test coverage. Tests are organized by their infrastructure requirements.

### Test Organization

```
┌─────────────────────────────────────────────────────────────────┐
│                        Test Categories                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Unit Tests (no feature flag)                                   │
│  ├─ Run on every commit                                         │
│  ├─ Fast execution, no external dependencies                    │
│  └─ Workflow: ci.yml (test job)                                 │
│                                                                  │
│  Integration Tests (--features integration-tests)               │
│  ├─ Mock-based integration tests                                │
│  ├─ Tests: p2p_integration, e2e_job_flow,                       │
│  │         ephemeral_network_isolation                          │
│  ├─ No Firecracker/Unikraft required                            │
│  └─ Workflow: ci.yml (test job)                                 │
│                                                                  │
│  E2E Tests (--features e2e-tests)                               │
│  ├─ Full system tests with real infrastructure                  │
│  ├─ Tests: firecracker_unikraft_executor_integration,           │
│  │         unikraft_build, TypeScript SDK tests                 │
│  ├─ Requires: Firecracker, Kraft, prebuilt kernels, KVM         │
│  └─ Workflow: e2e-test.yml                                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Workflow Details

#### `ci.yml` - Continuous Integration

Runs on every push and pull request to `main`.

**Jobs:**
- **check** - `cargo check` for compilation errors
- **test** - Unit and integration tests (no heavy infrastructure)
- **fmt** - Code formatting with `rustfmt`
- **clippy** - Linter checks

**What runs here:**
```bash
# Unit tests (default)
cargo test --workspace

# Integration tests (mocks only, no Firecracker)
cargo test --workspace --features integration-tests
```

**Dependencies installed:**
- ✅ Rust toolchain
- ❌ No Firecracker (not needed)
- ❌ No prebuilt kernels (not needed)

#### `e2e-test.yml` - End-to-End Tests

Runs on `workflow_dispatch` (manual trigger).

**Job Flow:**
```
1. build-kernels (via kernel-build.yml)
   └─ Builds Python, Node.js, Bun kernels
   └─ Uploads artifacts

2. e2e-test
   ├─ Downloads kernel artifacts
   ├─ Sets up system (Firecracker, Kraft, KVM)
   ├─ Builds worker binary + Anchor program
   ├─ Runs Rust E2E tests (--features e2e-tests)
   └─ Runs TypeScript E2E tests
```

**What runs here:**
```bash
# Rust tests requiring real Firecracker + kernels
export GRAPHENE_KERNEL_CACHE="$HOME/.graphene/cache/kernels"
cargo test -p graphene_node --features e2e-tests

# TypeScript SDK tests
cd sdks/node
bun test ./tests/*.e2e.test.ts
```

**Dependencies installed:**
- ✅ Firecracker v1.11.0
- ✅ Kraft CLI (latest)
- ✅ Prebuilt Unikraft kernels
- ✅ KVM enabled
- ✅ Solana + Anchor CLI
- ✅ System dependencies (cpio, e2fsprogs, libudev-dev)

#### `kernel-build.yml` - Kernel Builds

Builds Unikraft kernels for all supported runtimes.

**Triggers:**
- Changes to `kernels/**` directory
- Manual `workflow_dispatch`
- Called by `e2e-test.yml`

**Process:**
1. Generate build matrix from `kernels/kernel-matrix.toml`
2. Build each runtime/version combination with Kraft
3. Upload kernel binaries as artifacts
4. (On `main` branch) Create GitHub release with kernels

**Matrix:**
```toml
[runtimes.python]
versions = ["3.10", "3.11", "3.12"]

[runtimes.node]
versions = ["20", "21", "22"]

[runtimes.bun]
versions = ["1.1"]
```

### Shared Actions

#### `.github/actions/setup-system`

Composite action used by workflows needing Firecracker/Kraft/KVM:

```yaml
- name: Setup system dependencies
  uses: ./.github/actions/setup-system
```

**What it does:**
1. Enables KVM permissions via udev rules
2. Verifies KVM availability (fails if not accessible)
3. Installs system dependencies (cpio, e2fsprogs, libudev-dev)
4. Installs Firecracker v1.11.0
5. Installs Kraft CLI (latest release)

**Hard failures:**
- ❌ Fails immediately if `/dev/kvm` is not available
- ❌ No conditional skipping - KVM is mandatory

### Running Workflows Locally

#### CI Tests (Fast)
```bash
# Same as ci.yml test job
cargo test --workspace --features integration-tests
```

#### E2E Tests (Requires Infrastructure)
```bash
# 1. Build kernels first
cd kernels/python/3.12
kraft build --plat fc --arch x86_64
kraft pkg pull -w .unikraft/pkg --plat fc --arch x86_64 "unikraft.org/python:3.12"
mkdir -p ~/.graphene/cache/kernels
cp .unikraft/pkg/unikraft/bin/kernel ~/.graphene/cache/kernels/python-3.12_fc-x86_64

# 2. Run E2E tests
export GRAPHENE_KERNEL_CACHE="$HOME/.graphene/cache/kernels"
cargo test -p graphene_node --features e2e-tests
```

### Feature Flags Reference

| Feature Flag | Tests Included | Infrastructure Needed | Workflow |
|--------------|----------------|----------------------|----------|
| _(none)_ | Unit tests | None | ci.yml |
| `integration-tests` | P2P, job flow, network isolation (mocked) | None | ci.yml |
| `e2e-tests` | Firecracker executor, Unikraft builds | Firecracker, Kraft, kernels, KVM | e2e-test.yml |
| `tpm2-tools` | TPM attestation with CLI fallback | TPM2 tools | (optional) |

### Test File Naming Convention

```
crates/node/tests/
├── unit tests (no suffix) - Run without feature flags
├── *_integration.rs      - Use feature = "integration-tests"
├── e2e/                  - Use feature = "e2e-tests"
│   ├── mod.rs
│   └── unikraft_build.rs
└── firecracker_unikraft_executor_integration.rs - Use feature = "e2e-tests"
```

---

## Building the Native Node SDK

The `@graphene/sdk` package depends on `@graphene/sdk-native`, a Rust NAPI module providing cryptographic primitives and protocol serialization. Pre-built binaries are available for common platforms, but you may need to build from source for development or unsupported platforms.

### Prerequisites

- **Rust 1.70+** with the target platform toolchain
- **Node.js 18+** or **Bun**
- **Build dependencies** (already covered in E2E setup above):
  - `build-essential` (Linux)
  - `pkg-config` and `libssl-dev` (Linux)
  - Xcode Command Line Tools (macOS)

### Building from Source

```bash
cd sdks/node/native

# Install NAPI CLI and dependencies
bun install   # or: npm install

# Build release binary (creates .node file for your platform)
bun run build   # or: npm run build

# Build debug binary (faster compilation, slower runtime)
bun run build:debug
```

After building, the native module (e.g., `sdk-native.darwin-arm64.node`) will be created in the `sdks/node/native/` directory.

### Running Native Module Tests

```bash
cd sdks/node/native
bun run test   # or: npm test
```

### Cross-Compilation

The native SDK uses `openssl = { features = ["vendored"] }` to compile OpenSSL from source, enabling cross-compilation without system OpenSSL headers.

To build for a different target:

```bash
# Add the target toolchain
rustup target add aarch64-unknown-linux-gnu

# Build for that target
bun run build -- --target aarch64-unknown-linux-gnu
```

### Troubleshooting Native Builds

#### "error: linker `cc` not found"

Install build essentials:
```bash
# Linux
sudo apt-get install build-essential

# macOS
xcode-select --install
```

#### OpenSSL errors during build

The native SDK vendors OpenSSL, but if you see errors, ensure `pkg-config` is installed:
```bash
sudo apt-get install pkg-config libssl-dev
```

#### NAPI version mismatch

Ensure Node.js 18+ is installed. The native module requires NAPI version 9.

---

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GRAPHENE_WORKER_BINARY` | Path to graphene-worker binary | Auto-detected |
| `GRAPHENE_KERNEL_CACHE` | Path to kernel cache directory | `~/.graphene/cache/kernels` |
| `KVM_AVAILABLE` | Set to `false` to skip KVM-dependent tests | `true` |

### Troubleshooting

#### "KVM is not available"

- **Linux**: Ensure your CPU supports virtualization (Intel VT-x/AMD-V) and it's enabled in BIOS
- **macOS**: Use Lima with nested virtualization (see above)
- **VM/Cloud**: Check if your provider supports nested virtualization (Hetzner dedicated servers, GCP N2 instances, etc.)

#### "No default signer found"

Run `solana-keygen new --no-bip39-passphrase` to generate a keypair.

#### Program deployment fails

Ensure the Anchor program is built:

```bash
cd programs/graphene
anchor build
ls -la target/deploy/
```

#### Connection lost errors

The worker may have crashed. Check the worker logs and ensure all kernel images are available in the cache.
