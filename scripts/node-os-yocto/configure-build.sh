#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

MACHINE="${1:-graphene-node-x86_64}"
ensure_build_dirs

cd "$REPO_ROOT/poky"
set +u
source oe-init-build-env "$BUILD_DIR"
set -u

TOPDIR="${TOPDIR:-$PWD}"

cat > conf/bblayers.conf <<EOF
POKY_BBLAYERS_CONF_VERSION = "2"
BBPATH = "${TOPDIR}"
BBFILES ?= ""
BBLAYERS ?= " \\
    $REPO_ROOT/poky/meta \\
    $REPO_ROOT/poky/meta-poky \\
    $REPO_ROOT/poky/meta-yocto-bsp \\
    $REPO_ROOT/meta-rust-bin \\
    $REPO_ROOT/node-os/yocto/meta-graphene \\
"
EOF

cat > conf/local.conf <<EOF
MACHINE = "${MACHINE}"
BB_NUMBER_THREADS = "16"
PARALLEL_MAKE = "-j 16"
DL_DIR = "${BUILD_DIR}/downloads"
SSTATE_DIR = "${BUILD_DIR}/sstate-cache"
TMPDIR = "${BUILD_DIR}/tmp"
PACKAGE_CLASSES = "package_rpm"
INHERIT += "create-spdx"
INHERIT += "externalsrc"
EXTERNALSRC:pn-graphene-node = "${REPO_ROOT}"
EXTERNALSRC_BUILD:pn-graphene-node = "${REPO_ROOT}/target"
SRCREV:pn-graphene-node = "$(git -C "$REPO_ROOT" rev-parse HEAD)"
GRAPHENE_KERNEL_BRANCH = "${KERNEL_BRANCH}"
# meta-rust-bin overrides
BBMASK += "poky/meta/recipes-devtools/rust"
EOF
