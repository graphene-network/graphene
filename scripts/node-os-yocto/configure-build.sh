#!/usr/bin/env bash
set -euo pipefail
set +u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
set +u

MACHINE="${1:-graphene-node-x86_64}"
ensure_build_dirs

cd "$YOCTO_TOP"
export BBSERVER="${BBSERVER:-}"
export ZSH_NAME="${ZSH_NAME:-}"
if [[ -n "${YOCTO_TEMPLATECONF}" ]]; then
  TEMPLATECONF="${YOCTO_TEMPLATECONF}" source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
else
  source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
fi
set -u

TOPDIR="${TOPDIR:-$PWD}"

if [[ "$YOCTO_MODE" == "layers" ]]; then
  CORE_LAYER="${YOCTO_LAYERS_DIR}/openembedded-core/meta"
  POKY_LAYER="${YOCTO_LAYERS_DIR}/meta-yocto/meta-poky"
  BSP_LAYER="${YOCTO_LAYERS_DIR}/meta-yocto/meta-yocto-bsp"
  RUST_BBMASK="${YOCTO_LAYERS_DIR}/openembedded-core/meta/recipes-devtools/rust"
else
  CORE_LAYER="$REPO_ROOT/poky/meta"
  POKY_LAYER="$REPO_ROOT/poky/meta-poky"
  BSP_LAYER="$REPO_ROOT/poky/meta-yocto-bsp"
  RUST_BBMASK="$REPO_ROOT/poky/meta/recipes-devtools/rust"
fi

cat > conf/bblayers.conf <<EOF
POKY_BBLAYERS_CONF_VERSION = "2"
BBPATH = "${TOPDIR}"
BBFILES ?= ""
BBLAYERS ?= " \\
    ${CORE_LAYER} \\
    ${POKY_LAYER} \\
    ${BSP_LAYER} \\
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
BBMASK += "${RUST_BBMASK}"
EOF
