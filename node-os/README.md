# Graphene Node OS

Minimal, hardened Linux host OS for Graphene Network nodes.

## Overview

This is the **HOST operating system** that runs on bare metal or VMs. It provides:
- Firecracker hypervisor for running unikernel VMs
- `graphene-node` worker daemon
- Minimal attack surface (no shell, no package manager)
- dm-verity integrity verification (Phase 2)
- TPM-based attestation (Phase 2)

## Building

Graphene Node OS is built with Yocto for production-quality images with SBOM generation.

```bash
# Clone Yocto layers (Whinlatter / Yocto 5.3)
mkdir -p layers
git clone -b yocto-5.3 https://git.openembedded.org/bitbake layers/bitbake
git clone -b yocto-5.3 https://git.openembedded.org/openembedded-core layers/openembedded-core
git clone -b yocto-5.3 https://git.yoctoproject.org/meta-yocto layers/meta-yocto

# Setup environment
TEMPLATECONF="$(pwd)/layers/meta-yocto/meta-poky/conf/templates/default" \
  source layers/openembedded-core/oe-init-build-env ../build

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

| Feature | Status |
|---------|--------|
| No shell binaries | ✅ |
| No SSH server | ✅ |
| Stripped binaries | ✅ |
| Stack protector | ✅ |
| RELRO/NOW | ✅ |
| SPDX SBOM | ✅ |
| dm-verity | Phase 2 |
| TPM attestation | Phase 2 |

## CI/CD and Releases

### Build Triggers

| Trigger | What Runs | Duration |
|---------|-----------|----------|
| PR to `node-os/**` | Validation only (`bitbake -p`) | ~5-10 min |
| Tag `node-os-v*` | Full build + GitHub Release | ~60-90 min |
| Weekly (Sun 2am UTC) | Full build (drift detection) | ~60-90 min |
| Manual dispatch | Full build | ~60-90 min |

### Creating a Release

```bash
# Tag with semantic version
git tag node-os-v0.1.0
git push origin node-os-v0.1.0
```

This triggers:
1. Full Yocto build on `ubicloud-standard-16`
2. Shell removal verification
3. Image size check (<50MB target)
4. Artifact upload (`.wic.gz`, `.ext4`, SBOM)
5. GitHub Release creation
6. sstate cache sync to S3

### Manual Build

Trigger via GitHub Actions UI:
1. Go to Actions → "Build Node OS (Yocto)"
2. Click "Run workflow"
3. Select machine target
4. Optionally skip sstate cache

### Version Naming

- `node-os-v{major}.{minor}.{patch}` - Production releases
- `node-os-v{major}.{minor}.{patch}-rc{n}` - Release candidates
- `node-os-v{major}.{minor}.{patch}-alpha{n}` - Alpha builds

## Management

Since there's no shell, nodes are managed via the Iroh-based management API:

```bash
# Remote management
graphenectl --node prod-1 status
graphenectl --node prod-1 apply -f node-config.toml
graphenectl --node prod-1 drain
```

See `crates/ctl/` for the management CLI.

## References

- [WHITEPAPER.md](../docs/WHITEPAPER.md) - Graphene Network architecture
- [GitHub Issue #103](https://github.com/marcus-sa/graphene/issues/103) - Node OS tracking
