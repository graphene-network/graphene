# Graphene Node OS Image
# SPDX-License-Identifier: Apache-2.0
#
# Minimal, hardened Linux image for Graphene Network nodes.
# This is the HOST OS that runs Firecracker VMs.

SUMMARY = "Graphene Node OS - Minimal hardened image"
LICENSE = "Apache-2.0"

inherit core-image

# Inherit security hardening and SBOM generation
inherit graphene-hardening
inherit create-spdx

# dm-verity for rootfs integrity (Phase 2)
# inherit dm-verity-img

# Core packages - minimal set only
IMAGE_INSTALL = " \
    packagegroup-core-boot \
    kernel-modules \
    kmod \
    ${CORE_IMAGE_EXTRA_INSTALL} \
"

# Graphene-specific packages
IMAGE_INSTALL += " \
    graphene-node \
    firecracker \
"

# Tools required by graphene-node for drive creation
# Note: These are called directly via execve(), not via shell
IMAGE_INSTALL += " \
    e2fsprogs \
    tar \
"

# No debug features in production
IMAGE_FEATURES:remove = "debug-tweaks ssh-server-openssh"

# No package management (immutable OS)
IMAGE_FEATURES:remove = "package-management"

# Post-process: Strip shells and harden
# Note: Function names avoid underscores that BitBake might interpret as old override syntax
ROOTFS_POSTPROCESS_COMMAND:append = " graphene_stripshells; graphene_hardenrootfs;"

# Remove all shell binaries
graphene_stripshells() {
    # Remove shell binaries
    rm -f ${IMAGE_ROOTFS}/bin/sh \
          ${IMAGE_ROOTFS}/bin/bash \
          ${IMAGE_ROOTFS}/bin/ash \
          ${IMAGE_ROOTFS}/bin/dash \
          ${IMAGE_ROOTFS}/usr/bin/sh \
          ${IMAGE_ROOTFS}/usr/bin/bash \
          2>/dev/null || true

    # Update /etc/passwd to use nologin
    if [ -f "${IMAGE_ROOTFS}/etc/passwd" ]; then
        sed -i 's|:/bin/sh$|:/sbin/nologin|g' ${IMAGE_ROOTFS}/etc/passwd
        sed -i 's|:/bin/bash$|:/sbin/nologin|g' ${IMAGE_ROOTFS}/etc/passwd
        sed -i 's|:/bin/ash$|:/sbin/nologin|g' ${IMAGE_ROOTFS}/etc/passwd
    fi

    # Verify no shells remain
    for shell in sh bash ash dash zsh; do
        if [ -f "${IMAGE_ROOTFS}/bin/${shell}" ] || [ -f "${IMAGE_ROOTFS}/usr/bin/${shell}" ]; then
            bbfatal "Shell binary found after removal: ${shell}"
        fi
    done

    bbnote "Shell binaries removed successfully"
}

# Additional security hardening
graphene_hardenrootfs() {
    # Create Graphene directories
    install -d ${IMAGE_ROOTFS}/etc/graphene
    install -d ${IMAGE_ROOTFS}/var/lib/graphene
    install -d ${IMAGE_ROOTFS}/var/lib/firecracker
    install -d ${IMAGE_ROOTFS}/var/log/graphene

    # Set restrictive permissions
    chmod 700 ${IMAGE_ROOTFS}/etc/graphene
    chmod 700 ${IMAGE_ROOTFS}/var/lib/graphene
    chmod 700 ${IMAGE_ROOTFS}/var/lib/firecracker

    # Remove unnecessary files
    rm -rf ${IMAGE_ROOTFS}/usr/share/man \
           ${IMAGE_ROOTFS}/usr/share/doc \
           ${IMAGE_ROOTFS}/usr/share/info \
           ${IMAGE_ROOTFS}/var/cache/* \
           2>/dev/null || true

    # Remove getty and login utilities
    rm -f ${IMAGE_ROOTFS}/sbin/getty \
          ${IMAGE_ROOTFS}/bin/login \
          ${IMAGE_ROOTFS}/usr/bin/passwd \
          2>/dev/null || true

    # Create os-release
    cat > ${IMAGE_ROOTFS}/etc/os-release << 'EOF'
NAME="Graphene Node OS"
ID=graphene
VERSION_ID="${PV}"
PRETTY_NAME="Graphene Node OS ${PV}"
HOME_URL="https://graphene.network"
EOF

    bbnote "Rootfs hardening complete"
}

# Disable root password
EXTRA_IMAGE_FEATURES:remove = "allow-root-login"

# Minimal locale
IMAGE_LINGUAS = ""

# SPDX SBOM configuration
SPDX_INCLUDE_SOURCES = "0"
SPDX_INCLUDE_TIMESTAMPS = "1"
