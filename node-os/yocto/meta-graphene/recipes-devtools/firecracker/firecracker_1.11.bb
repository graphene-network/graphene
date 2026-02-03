# Firecracker MicroVM Manager
# SPDX-License-Identifier: Apache-2.0

SUMMARY = "Firecracker - Secure and fast microVMs for serverless computing"
HOMEPAGE = "https://firecracker-microvm.github.io/"
LICENSE = "Apache-2.0"
LIC_FILES_CHKSUM = "file://LICENSE;md5=3b83ef96387f14655fc854ddc3c6bd57"

# Version
PV = "1.11.0"

# Architecture-specific downloads
SRC_URI:x86-64 = "https://github.com/firecracker-microvm/firecracker/releases/download/v${PV}/firecracker-v${PV}-x86_64.tgz"
SRC_URI:aarch64 = "https://github.com/firecracker-microvm/firecracker/releases/download/v${PV}/firecracker-v${PV}-aarch64.tgz"

SRC_URI[x86-64.sha256sum] = "FIXME_ADD_CHECKSUM"
SRC_URI[aarch64.sha256sum] = "FIXME_ADD_CHECKSUM"

# Only supported on x86_64 and aarch64
COMPATIBLE_HOST = "(x86_64|aarch64).*-linux"

S = "${WORKDIR}"

# Pre-built binaries, no compilation needed
do_compile[noexec] = "1"

do_install() {
    install -d ${D}${bindir}

    # Install architecture-specific binaries
    if [ "${TARGET_ARCH}" = "x86_64" ]; then
        install -m 0755 ${S}/release-v${PV}-x86_64/firecracker-v${PV}-x86_64 ${D}${bindir}/firecracker
        install -m 0755 ${S}/release-v${PV}-x86_64/jailer-v${PV}-x86_64 ${D}${bindir}/jailer
    elif [ "${TARGET_ARCH}" = "aarch64" ]; then
        install -m 0755 ${S}/release-v${PV}-aarch64/firecracker-v${PV}-aarch64 ${D}${bindir}/firecracker
        install -m 0755 ${S}/release-v${PV}-aarch64/jailer-v${PV}-aarch64 ${D}${bindir}/jailer
    fi

    # Create systemd service directory
    install -d ${D}${systemd_system_unitdir}
}

# Skip QA checks for pre-built binaries
INSANE_SKIP:${PN} = "already-stripped ldflags"

# Runtime dependencies
RDEPENDS:${PN} = "kernel-module-kvm"
