# OpenCapsule Node Binary
# SPDX-License-Identifier: Apache-2.0

SUMMARY = "OpenCapsule Network Node - Worker daemon for executing unikernel jobs"
HOMEPAGE = "https://opencapsule.dev"
LICENSE = "AGPL-3.0-only"
LIC_FILES_CHKSUM = "file://LICENSE;md5=eb1e647870add0502f8f010b19de32af"

# Fetch from git - uses the opencapsule repository
# Branch can be overridden via OPENCAPSULE_GIT_BRANCH (e.g., for CI on PR branches)
OPENCAPSULE_GIT_BRANCH ?= "main"
SRC_URI = "git://github.com/opencapsule/opencapsule.git;protocol=https;branch=${OPENCAPSULE_GIT_BRANCH} \
           file://node-config.toml \
           "
SRCREV = "${AUTOREV}"
PV = "0.1.0+git${SRCPV}"

# Rust build - use meta-rust-bin's prebuilt toolchain (supports newer Rust versions)
inherit cargo_bin

# Build dependencies
DEPENDS = "openssl"

# Environment variables for attestation (set by CI)
OPENCAPSULE_VERITY_ROOT ?= ""
OPENCAPSULE_PCR_0 ?= ""
OPENCAPSULE_PCR_7 ?= ""
OPENCAPSULE_PLATFORM_ID ?= "opencapsule-os-${PV}"
OPENCAPSULE_BUILD_TIME ?= ""

# Pass attestation values to cargo
CARGO_BUILD_FLAGS = "--release --package opencapsule_node --bin opencapsule-worker"

# Enable network access for do_compile (required for cargo to fetch dependencies)
# See: https://github.com/rust-embedded/meta-rust-bin#use-with-yocto-release-40-kirkstone-and-above
do_compile[network] = "1"

do_compile:prepend() {
    # Set environment variables for build-time attestation embedding
    export OPENCAPSULE_VERITY_ROOT="${OPENCAPSULE_VERITY_ROOT}"
    export OPENCAPSULE_PCR_0="${OPENCAPSULE_PCR_0}"
    export OPENCAPSULE_PCR_7="${OPENCAPSULE_PCR_7}"
    export OPENCAPSULE_PLATFORM_ID="${OPENCAPSULE_PLATFORM_ID}"
    export OPENCAPSULE_BUILD_TIME="${OPENCAPSULE_BUILD_TIME}"
}

do_install() {
    install -d ${D}${bindir}
    # Binary is built as opencapsule-worker, install as opencapsule-node for consistency
    # CARGO_BINDIR is set by cargo_bin class to ${B}/${RUST_TARGET}/${profile}/
    install -m 0755 ${CARGO_BINDIR}/opencapsule-worker ${D}${bindir}/opencapsule-node

    # Install default configuration
    install -d ${D}${sysconfdir}/opencapsule
    install -m 0644 ${WORKDIR}/node-config.toml ${D}${sysconfdir}/opencapsule/node-config.toml

    # Install systemd service
    install -d ${D}${systemd_system_unitdir}
    cat > ${D}${systemd_system_unitdir}/opencapsule-node.service << 'EOF'
[Unit]
Description=OpenCapsule Network Node
After=network.target
Wants=network.target

[Service]
Type=simple
ExecStart=/usr/bin/opencapsule-node --config /etc/opencapsule/node-config.toml
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/opencapsule /var/lib/firecracker /var/log/opencapsule

[Install]
WantedBy=multi-user.target
EOF
}

# Runtime dependencies
RDEPENDS:${PN} = "firecracker kernel-module-kvm"

# Configuration files
CONFFILES:${PN} = "${sysconfdir}/opencapsule/node-config.toml"

# Enable systemd service
inherit systemd
SYSTEMD_SERVICE:${PN} = "opencapsule-node.service"
SYSTEMD_AUTO_ENABLE:${PN} = "enable"
