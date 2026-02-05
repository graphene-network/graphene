# Firecracker MicroVM Manager
# SPDX-License-Identifier: Apache-2.0

SUMMARY = "Firecracker - Secure and fast microVMs for serverless computing"
HOMEPAGE = "https://firecracker-microvm.github.io/"
LICENSE = "Apache-2.0"
LIC_FILES_CHKSUM = "file://LICENSE;md5=3b83ef96387f14655fc854ddc3c6bd57"

# Version
PV = "1.11.0"

# Fetch LICENSE file from the source repository
SRC_URI = "https://raw.githubusercontent.com/firecracker-microvm/firecracker/v${PV}/LICENSE;name=license"
SRC_URI[license.sha256sum] = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"

# Architecture-specific downloads with named sources for checksums
SRC_URI:append:x86-64 = " https://github.com/firecracker-microvm/firecracker/releases/download/v${PV}/firecracker-v${PV}-x86_64.tgz;name=fc-x86"
SRC_URI:append:aarch64 = " https://github.com/firecracker-microvm/firecracker/releases/download/v${PV}/firecracker-v${PV}-aarch64.tgz;name=fc-aarch64"

SRC_URI[fc-x86.sha256sum] = "38ad6fb34273b2fa616956237b15ea6e064cf21336b0d990d5de347b35b9328b"
SRC_URI[fc-aarch64.sha256sum] = "4b98f7cd669a772716fd1bef59c75188ba05a683bc0759ee4169eb351274fcb0"

# Only supported on x86_64 and aarch64
COMPATIBLE_HOST = "(x86_64|aarch64).*-linux"

# Pre-built binaries, no compilation needed
do_compile[noexec] = "1"

do_install() {
    install -d ${D}${bindir}

    # Install architecture-specific binaries
    if [ "${TARGET_ARCH}" = "x86_64" ]; then
        install -m 0755 ${WORKDIR}/release-v${PV}-x86_64/firecracker-v${PV}-x86_64 ${D}${bindir}/firecracker
        install -m 0755 ${WORKDIR}/release-v${PV}-x86_64/jailer-v${PV}-x86_64 ${D}${bindir}/jailer
    elif [ "${TARGET_ARCH}" = "aarch64" ]; then
        install -m 0755 ${WORKDIR}/release-v${PV}-aarch64/firecracker-v${PV}-aarch64 ${D}${bindir}/firecracker
        install -m 0755 ${WORKDIR}/release-v${PV}-aarch64/jailer-v${PV}-aarch64 ${D}${bindir}/jailer
    fi

}

# Skip QA checks for pre-built binaries
INSANE_SKIP:${PN} = "already-stripped ldflags"

# Runtime dependencies
RDEPENDS:${PN} = "kernel-module-kvm"
