# Graphene Node Binary
# SPDX-License-Identifier: Apache-2.0

SUMMARY = "Graphene Network Node - Worker daemon for executing unikernel jobs"
HOMEPAGE = "https://graphene.network"
LICENSE = "AGPL-3.0-only"
LIC_FILES_CHKSUM = "file://LICENSE;md5=eb1e647870add0502f8f010b19de32af"

# Fetch from git - uses the monad repository
# Branch can be overridden via GRAPHENE_GIT_BRANCH (e.g., for CI on PR branches)
GRAPHENE_GIT_BRANCH ?= "main"
SRC_URI = "git://github.com/marcus-sa/monad.git;protocol=https;branch=${GRAPHENE_GIT_BRANCH} \
           file://node-config.toml \
           "
SRCREV = "${AUTOREV}"
PV = "0.1.0+git${SRCPV}"

# Rust build - use meta-rust-bin's prebuilt toolchain (supports newer Rust versions)
inherit cargo_bin

# Build dependencies
DEPENDS = "openssl"

# Environment variables for attestation (set by CI)
GRAPHENE_VERITY_ROOT ?= ""
GRAPHENE_PCR_0 ?= ""
GRAPHENE_PCR_7 ?= ""
GRAPHENE_PLATFORM_ID ?= "graphene-os-${PV}"
GRAPHENE_BUILD_TIME ?= ""

# Pass attestation values to cargo
CARGO_BUILD_FLAGS = "--release --package monad_node --bin graphene-worker"

# Enable network access for do_compile (required for cargo to fetch dependencies)
# See: https://github.com/rust-embedded/meta-rust-bin#use-with-yocto-release-40-kirkstone-and-above
do_compile[network] = "1"

do_compile:prepend() {
    # Set environment variables for build-time attestation embedding
    export GRAPHENE_VERITY_ROOT="${GRAPHENE_VERITY_ROOT}"
    export GRAPHENE_PCR_0="${GRAPHENE_PCR_0}"
    export GRAPHENE_PCR_7="${GRAPHENE_PCR_7}"
    export GRAPHENE_PLATFORM_ID="${GRAPHENE_PLATFORM_ID}"
    export GRAPHENE_BUILD_TIME="${GRAPHENE_BUILD_TIME}"
}

do_install() {
    install -d ${D}${bindir}
    # Binary is built as graphene-worker, install as graphene-node for consistency
    # CARGO_BINDIR is set by cargo_bin class to ${B}/${RUST_TARGET}/${profile}/
    install -m 0755 ${CARGO_BINDIR}/graphene-worker ${D}${bindir}/graphene-node

    # Install default configuration
    install -d ${D}${sysconfdir}/graphene
    install -m 0644 ${WORKDIR}/node-config.toml ${D}${sysconfdir}/graphene/node-config.toml

    # Install systemd service
    install -d ${D}${systemd_system_unitdir}
    cat > ${D}${systemd_system_unitdir}/graphene-node.service << 'EOF'
[Unit]
Description=Graphene Network Node
After=network.target
Wants=network.target

[Service]
Type=simple
ExecStart=/usr/bin/graphene-node --config /etc/graphene/node-config.toml
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/graphene /var/lib/firecracker /var/log/graphene

[Install]
WantedBy=multi-user.target
EOF
}

# Runtime dependencies
RDEPENDS:${PN} = "firecracker kernel-module-kvm"

# Configuration files
CONFFILES:${PN} = "${sysconfdir}/graphene/node-config.toml"

# Enable systemd service
inherit systemd
SYSTEMD_SERVICE:${PN} = "graphene-node.service"
SYSTEMD_AUTO_ENABLE:${PN} = "enable"
