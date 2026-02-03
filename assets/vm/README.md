# Builder VM Artifacts

Binary artifacts for ephemeral builder VMs.

## Files

| File | Description | Size |
|------|-------------|------|
| `vmlinux-builder` | Firecracker-compatible Linux kernel | ~25MB |
| `rootfs-builder.ext4` | Alpine Linux rootfs with Kraft CLI | ~2GB |

## Building

```bash
# Build all artifacts (requires sudo for rootfs)
./scripts/build-vm-artifacts.sh

# Download kernel only
./scripts/build-vm-artifacts.sh kernel

# Create rootfs only
./scripts/build-vm-artifacts.sh rootfs

# Clean and rebuild
./scripts/build-vm-artifacts.sh --clean all
```

## Testing

```bash
# Test with Firecracker (requires /dev/kvm)
firecracker --no-api \
  --kernel assets/vm/vmlinux-builder \
  --root-drive assets/vm/rootfs-builder.ext4 \
  --boot-args "console=ttyS0 reboot=k panic=1 pci=off init=/init"
```

## Contents

### Kernel (vmlinux-builder)
- Firecracker CI kernel v5.10.225
- Minimal config for microVMs
- ~25MB uncompressed

### Rootfs (rootfs-builder.ext4)
- Alpine Linux 3.19 base
- Build tools: gcc, make, python3
- Kraft CLI for unikernel builds
- Custom `/init` script for build automation

## Init Script Flow

1. Mount `/proc`, `/sys`, `/dev`
2. Mount input drive (`/dev/vdb`) read-only to `/input`
3. Mount output drive (`/dev/vdc`) read-write to `/output`
4. Detect build type (Kraftfile or Dockerfile)
5. Run `kraft build` with appropriate options
6. Write exit code to `/output/exit_code`
7. Shutdown VM

## Drive Layout

| Drive | Device | Mount | Purpose |
|-------|--------|-------|---------|
| Root | vda | / | Builder rootfs |
| Input | vdb | /input | User code (Dockerfile, Kraftfile) |
| Output | vdc | /output | Build artifacts (.unik file) |

## CI Integration

Artifacts are built in CI and cached. See `.github/workflows/builder-artifacts.yml`.
