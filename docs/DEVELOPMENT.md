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
cd /Users/$(whoami)/Git/monad
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
cd /Users/$(whoami)/Git/monad
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
