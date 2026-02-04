# Development Guide

## Running E2E Tests

The E2E tests require Firecracker, which depends on KVM (Linux only). This guide covers running tests on different platforms.

### Linux (Native)

On Linux with KVM support:

```bash
# Install dependencies
sudo apt-get install -y e2fsprogs

# Install Firecracker
curl -fsSL "https://github.com/firecracker-microvm/firecracker/releases/download/v1.11.0/firecracker-v1.11.0-x86_64.tgz" -o firecracker.tgz
tar -xzf firecracker.tgz
sudo mv release-v1.11.0-x86_64/firecracker-v1.11.0-x86_64 /usr/local/bin/firecracker
sudo chmod +x /usr/local/bin/firecracker

# Install Solana CLI
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"

# Generate keypair for program deployment
solana-keygen new --no-bip39-passphrase

# Install Anchor CLI
cargo install --git https://github.com/coral-xyz/anchor --tag v0.32.1 anchor-cli

# Build and run tests
cd sdks/node
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

#### 6. Build and Run Tests

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

#### Stopping the VM

```bash
# From macOS host
limactl stop graphene

# To delete the VM entirely
limactl delete graphene
```

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
