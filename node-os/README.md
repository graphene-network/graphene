# Graphene Node OS

Minimal, hardened Linux host OS for Graphene Network nodes.

## Overview

This is the **HOST operating system** that runs on bare metal or VMs. It provides:
- Firecracker hypervisor for running unikernel VMs
- `graphene-node` worker daemon
- Minimal attack surface (no shell, no package manager)
- dm-verity integrity verification (Phase 2)
- TPM-based attestation (Phase 2)

## Build Systems

### Buildroot (Prototype)

Quick prototype for architecture validation.

```bash
# Clone buildroot
git clone https://github.com/buildroot/buildroot.git
cd buildroot

# Configure
make BR2_EXTERNAL=/path/to/node-os/buildroot/external graphene_node_defconfig

# Build
make
```

Output: `output/images/rootfs.ext4` (~48MB)

### Yocto (Production)

Production-quality build with SBOM generation.

```bash
# Clone Poky
git clone -b kirkstone https://git.yoctoproject.org/poky
cd poky

# Setup environment
source oe-init-build-env ../build

# Add meta-graphene layer
bitbake-layers add-layer /path/to/node-os/yocto/meta-graphene

# Configure machine
echo 'MACHINE = "graphene-node-x86_64"' >> conf/local.conf

# Build
bitbake graphene-node-image
```

Output: `tmp/deploy/images/graphene-node-x86_64/graphene-node-image-*.wic.gz`

## Directory Structure

```
node-os/
├── os-matrix.toml          # Version matrix configuration
├── buildroot/              # Buildroot prototype
│   ├── configs/            # Defconfig files
│   ├── board/graphene/     # Board-specific files
│   └── external/           # External packages
└── yocto/
    └── meta-graphene/      # Yocto layer
        ├── conf/           # Layer and machine configs
        ├── recipes-core/   # Core image recipe
        ├── recipes-devtools/   # Firecracker
        ├── recipes-graphene/   # graphene-node
        ├── recipes-security/   # dm-verity, TPM
        ├── classes/        # Hardening bbclass
        └── wic/            # Disk image layouts
```

## Security Features

| Feature | Buildroot | Yocto |
|---------|-----------|-------|
| No shell binaries | ✅ | ✅ |
| No SSH server | ✅ | ✅ |
| Stripped binaries | ✅ | ✅ |
| Stack protector | ✅ | ✅ |
| RELRO/NOW | ⚠️ partial | ✅ |
| dm-verity | ❌ | ⚠️ Phase 2 |
| TPM attestation | ❌ | ⚠️ Phase 2 |
| SPDX SBOM | ❌ | ✅ |

## Management

Since there's no shell, nodes are managed via the Iroh-based management API:

```bash
# Remote management
graphenectl --node prod-1 status
graphenectl --node prod-1 apply -f node-config.yaml
graphenectl --node prod-1 drain
```

See `crates/ctl/` for the management CLI.

## References

- [WHITEPAPER.md](../docs/WHITEPAPER.md) - Graphene Network architecture
- [GitHub Issue #103](https://github.com/marcus-sa/monad/issues/103) - Yocto/Buildroot tracking
