#!/bin/bash
# Build VM artifacts for ephemeral builder
# SPDX-License-Identifier: Apache-2.0
#
# Creates:
# - assets/vm/vmlinux-builder (Firecracker-compatible kernel)
# - assets/vm/rootfs-builder.ext4 (Alpine-based rootfs with Kraft CLI)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ASSETS_DIR="$PROJECT_ROOT/assets/vm"
ROOTFS_SIZE_MB=2048
ALPINE_VERSION="3.19"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

check_dependencies() {
    log_info "Checking dependencies..."

    local missing=()
    for cmd in curl mkfs.ext4 sudo; do
        if ! command -v "$cmd" &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing dependencies: ${missing[*]}"
        exit 1
    fi

    # Check if running as root or can use sudo
    if [ "$EUID" -ne 0 ] && ! sudo -n true 2>/dev/null; then
        log_warn "This script requires sudo for mounting filesystems"
    fi
}

download_kernel() {
    log_info "Downloading Firecracker-compatible kernel..."

    local kernel_url="https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.10/x86_64/vmlinux-5.10.225"
    local kernel_path="$ASSETS_DIR/vmlinux-builder"

    if [ -f "$kernel_path" ]; then
        log_info "Kernel already exists at $kernel_path"
        return 0
    fi

    curl -fsSL -o "$kernel_path" "$kernel_url"
    chmod +x "$kernel_path"

    log_info "Kernel downloaded: $(du -h "$kernel_path" | cut -f1)"
}

create_rootfs() {
    log_info "Creating rootfs image (${ROOTFS_SIZE_MB}MB)..."

    local rootfs_path="$ASSETS_DIR/rootfs-builder.ext4"
    local mount_point="/tmp/builder-rootfs-$$"

    if [ -f "$rootfs_path" ]; then
        log_warn "Rootfs already exists. Remove it to rebuild: $rootfs_path"
        return 0
    fi

    # Create sparse image
    dd if=/dev/zero of="$rootfs_path" bs=1M count=0 seek="$ROOTFS_SIZE_MB" 2>/dev/null
    mkfs.ext4 -q "$rootfs_path"

    # Mount
    sudo mkdir -p "$mount_point"
    sudo mount "$rootfs_path" "$mount_point"

    # Cleanup on exit
    trap "sudo umount '$mount_point' 2>/dev/null || true; sudo rmdir '$mount_point' 2>/dev/null || true" EXIT

    bootstrap_alpine "$mount_point"
    install_build_tools "$mount_point"
    create_init_script "$mount_point"

    # Sync and unmount
    sync
    sudo umount "$mount_point"
    sudo rmdir "$mount_point"
    trap - EXIT

    log_info "Rootfs created: $(du -h "$rootfs_path" | cut -f1)"
}

bootstrap_alpine() {
    local mount_point="$1"

    log_info "Bootstrapping Alpine Linux ${ALPINE_VERSION}..."

    # Download and extract Alpine minirootfs
    local alpine_url="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/releases/x86_64/alpine-minirootfs-${ALPINE_VERSION}.0-x86_64.tar.gz"
    local tmp_tar="/tmp/alpine-minirootfs.tar.gz"

    curl -fsSL -o "$tmp_tar" "$alpine_url"
    sudo tar -xzf "$tmp_tar" -C "$mount_point"
    rm -f "$tmp_tar"

    # Setup resolv.conf for chroot networking
    sudo cp /etc/resolv.conf "$mount_point/etc/resolv.conf"

    # Setup APK repositories
    sudo tee "$mount_point/etc/apk/repositories" > /dev/null << EOF
https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/main
https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/community
EOF
}

install_build_tools() {
    local mount_point="$1"

    log_info "Installing build tools..."

    # Install packages via chroot
    sudo chroot "$mount_point" /bin/sh -c '
        apk update
        apk add --no-cache \
            bash curl wget git ca-certificates \
            python3 py3-pip \
            gcc musl-dev linux-headers make \
            tar gzip xz zstd \
            docker-cli
    '

    log_info "Installing Kraft CLI..."

    # Install kraft CLI
    sudo chroot "$mount_point" /bin/sh -c '
        # Install kraft via pip (more reliable than install script in chroot)
        pip3 install --break-system-packages kraft || {
            # Fallback: download binary directly
            KRAFT_VERSION=$(curl -s https://api.github.com/repos/unikraft/kraftkit/releases/latest | grep tag_name | cut -d\" -f4)
            curl -fsSL "https://github.com/unikraft/kraftkit/releases/download/${KRAFT_VERSION}/kraft_${KRAFT_VERSION#v}_linux_amd64.tar.gz" -o /tmp/kraft.tar.gz
            tar -xzf /tmp/kraft.tar.gz -C /usr/local/bin kraft
            rm /tmp/kraft.tar.gz
        }
    ' 2>/dev/null || log_warn "Kraft CLI installation may have issues in chroot"
}

create_init_script() {
    local mount_point="$1"

    log_info "Creating init script..."

    sudo tee "$mount_point/init" > /dev/null << 'INITEOF'
#!/bin/sh
# Ephemeral Builder Init Script
# SPDX-License-Identifier: Apache-2.0

set -e

# Mount essential filesystems
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev

# Create mount points
mkdir -p /input /output

# Wait for block devices
sleep 1

# Mount input drive (read-only) - contains Dockerfile, Kraftfile, code
if [ -b /dev/vdb ]; then
    mount /dev/vdb /input -o ro
else
    echo "[builder] ERROR: Input drive /dev/vdb not found"
    echo "1" > /dev/vdc/exit_code 2>/dev/null || true
    poweroff -f
fi

# Mount output drive (read-write) - for build artifacts
if [ -b /dev/vdc ]; then
    mount /dev/vdc /output
else
    echo "[builder] ERROR: Output drive /dev/vdc not found"
    poweroff -f
fi

# Start build
echo "[builder] Starting build at $(date)" | tee /output/build.log
cd /input

# Detect build type and run appropriate command
BUILD_EXIT=0

if [ -f "Kraftfile" ] || [ -f "Kraftfile.yaml" ] || [ -f "kraft.yaml" ]; then
    echo "[builder] Found Kraftfile, running kraft build" | tee -a /output/build.log

    # Run kraft build
    kraft build --plat fc --arch x86_64 -o /output/app.unik 2>&1 | tee -a /output/build.log
    BUILD_EXIT=${PIPESTATUS[0]}

elif [ -f "Dockerfile" ]; then
    echo "[builder] Found Dockerfile" | tee -a /output/build.log

    # Check for runtime hint in Dockerfile
    RUNTIME=$(grep -E "^#\s*unikraft-runtime:" Dockerfile | head -1 | cut -d: -f2 | tr -d ' ' || echo "")

    if [ -n "$RUNTIME" ]; then
        echo "[builder] Detected runtime: $RUNTIME" | tee -a /output/build.log
        # Generate Kraftfile from Dockerfile
        cat > /tmp/Kraftfile.yaml << EOF
spec: v0.6
runtime: unikraft.org/${RUNTIME}
rootfs: ./Dockerfile
cmd: ["/app/main"]
EOF
        kraft build --plat fc --arch x86_64 -o /output/app.unik -K /tmp/Kraftfile.yaml 2>&1 | tee -a /output/build.log
        BUILD_EXIT=${PIPESTATUS[0]}
    else
        echo "[builder] ERROR: Dockerfile found but no unikraft-runtime specified" | tee -a /output/build.log
        echo "[builder] Add '# unikraft-runtime: python:3.12' or similar to Dockerfile" | tee -a /output/build.log
        BUILD_EXIT=1
    fi
else
    echo "[builder] ERROR: No Dockerfile or Kraftfile found in /input" | tee -a /output/build.log
    ls -la /input | tee -a /output/build.log
    BUILD_EXIT=1
fi

# Write exit code
echo $BUILD_EXIT > /output/exit_code
echo "[builder] Build completed with exit code $BUILD_EXIT at $(date)" | tee -a /output/build.log

# List output
echo "[builder] Output files:" | tee -a /output/build.log
ls -la /output | tee -a /output/build.log

# Sync and shutdown
sync
sleep 1
echo "[builder] Shutting down..."
poweroff -f
INITEOF

    sudo chmod +x "$mount_point/init"
}

verify_artifacts() {
    log_info "Verifying artifacts..."

    local kernel_path="$ASSETS_DIR/vmlinux-builder"
    local rootfs_path="$ASSETS_DIR/rootfs-builder.ext4"

    local errors=0

    if [ ! -f "$kernel_path" ]; then
        log_error "Kernel not found: $kernel_path"
        errors=$((errors + 1))
    else
        log_info "Kernel: $(du -h "$kernel_path" | cut -f1)"
    fi

    if [ ! -f "$rootfs_path" ]; then
        log_error "Rootfs not found: $rootfs_path"
        errors=$((errors + 1))
    else
        log_info "Rootfs: $(du -h "$rootfs_path" | cut -f1)"
    fi

    if [ $errors -eq 0 ]; then
        log_info "All artifacts created successfully!"
        echo ""
        echo "Assets directory:"
        ls -lh "$ASSETS_DIR"
    else
        log_error "Some artifacts failed to build"
        return 1
    fi
}

usage() {
    cat << EOF
Usage: $0 [OPTIONS] [COMMAND]

Commands:
  all       Build all artifacts (default)
  kernel    Download kernel only
  rootfs    Create rootfs only
  verify    Verify existing artifacts

Options:
  -h, --help    Show this help message
  --clean       Remove existing artifacts before building

Examples:
  $0                    # Build all artifacts
  $0 --clean all        # Clean and rebuild
  $0 kernel             # Download kernel only
EOF
}

main() {
    local clean=false
    local command="all"

    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                usage
                exit 0
                ;;
            --clean)
                clean=true
                shift
                ;;
            all|kernel|rootfs|verify)
                command="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done

    check_dependencies
    mkdir -p "$ASSETS_DIR"

    if [ "$clean" = true ]; then
        log_info "Cleaning existing artifacts..."
        rm -f "$ASSETS_DIR/vmlinux-builder" "$ASSETS_DIR/rootfs-builder.ext4"
    fi

    case $command in
        all)
            download_kernel
            create_rootfs
            verify_artifacts
            ;;
        kernel)
            download_kernel
            ;;
        rootfs)
            create_rootfs
            ;;
        verify)
            verify_artifacts
            ;;
    esac
}

main "$@"
