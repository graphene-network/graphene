#!/usr/bin/env bash
set -euo pipefail

SCRIPT_SOURCE="${BASH_SOURCE[0]:-$0}"
SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"

if git -C "$SCRIPT_DIR/../.." rev-parse --show-toplevel >/dev/null 2>&1; then
  REPO_ROOT="$(git -C "$SCRIPT_DIR/../.." rev-parse --show-toplevel)"
else
  REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

export REPO_ROOT
export SSTATE_BUCKET="${SSTATE_BUCKET:-graphene-sstate}"
export BUILD_DIR="${BUILD_DIR:-/tmp/yocto-build}"
export NODE_OS_MATRIX="${NODE_OS_MATRIX:-$REPO_ROOT/node-os/os-matrix.toml}"
export YOCTO_TOP="${YOCTO_TOP:-}"
export YOCTO_LAYERS_DIR="${YOCTO_LAYERS_DIR:-}"
export YOCTO_INIT_ENV="${YOCTO_INIT_ENV:-}"
export YOCTO_TEMPLATECONF="${YOCTO_TEMPLATECONF:-}"
export YOCTO_BRANCH="${YOCTO_BRANCH:-}"
export YOCTO_REF="${YOCTO_REF:-}"
export YOCTO_OECORE_REF="${YOCTO_OECORE_REF:-}"
export YOCTO_METAYOCTO_REF="${YOCTO_METAYOCTO_REF:-}"
export YOCTO_BITBAKE_REF="${YOCTO_BITBAKE_REF:-}"
export YOCTO_STRICT="${YOCTO_STRICT:-1}"
export YOCTO_MODE="${YOCTO_MODE:-}"

function load_node_os_config() {
  if [[ ! -f "$NODE_OS_MATRIX" ]]; then
    echo "ERROR: Matrix file not found at $NODE_OS_MATRIX"
    return 1
  fi

  YOCTO_RELEASE=$(grep -m1 'yocto_version' "$NODE_OS_MATRIX" | cut -d'"' -f2)
  KERNEL_BRANCH=$(grep -m1 'linux_kernel_branch' "$NODE_OS_MATRIX" | cut -d'"' -f2)
  RUST_VERSION=$(grep -m1 '^rust = ' "$NODE_OS_MATRIX" | cut -d'"' -f2)

  export YOCTO_RELEASE KERNEL_BRANCH RUST_VERSION
}

load_node_os_config

function configure_yocto_paths() {
  case "${YOCTO_RELEASE}" in
    whinlatter)
      YOCTO_MODE="layers"
      YOCTO_REF="${YOCTO_REF:-yocto-5.3}"
      YOCTO_OECORE_REF="${YOCTO_OECORE_REF:-$YOCTO_REF}"
      YOCTO_METAYOCTO_REF="${YOCTO_METAYOCTO_REF:-$YOCTO_REF}"
      YOCTO_BITBAKE_REF="${YOCTO_BITBAKE_REF:-$YOCTO_REF}"
      YOCTO_TOP="${YOCTO_TOP:-$REPO_ROOT/yocto}"
      YOCTO_LAYERS_DIR="${YOCTO_LAYERS_DIR:-$YOCTO_TOP/layers}"
      YOCTO_INIT_ENV="${YOCTO_INIT_ENV:-$YOCTO_LAYERS_DIR/openembedded-core/oe-init-build-env}"
      YOCTO_TEMPLATECONF="${YOCTO_TEMPLATECONF:-$YOCTO_LAYERS_DIR/meta-yocto/meta-poky/conf/templates/default}"
      ;;
    *)
      YOCTO_MODE="poky"
      YOCTO_TOP="${YOCTO_TOP:-$REPO_ROOT/poky}"
      YOCTO_LAYERS_DIR="${YOCTO_LAYERS_DIR:-$YOCTO_TOP}"
      YOCTO_INIT_ENV="${YOCTO_INIT_ENV:-$YOCTO_TOP/oe-init-build-env}"
      YOCTO_TEMPLATECONF="${YOCTO_TEMPLATECONF:-}"
      YOCTO_BRANCH="${YOCTO_BRANCH:-$YOCTO_RELEASE}"
      ;;
  esac

  export YOCTO_MODE YOCTO_BRANCH YOCTO_REF YOCTO_OECORE_REF YOCTO_METAYOCTO_REF YOCTO_BITBAKE_REF YOCTO_STRICT
  export YOCTO_TOP YOCTO_LAYERS_DIR YOCTO_INIT_ENV YOCTO_TEMPLATECONF
}

configure_yocto_paths

function ensure_build_dirs() {
  mkdir -p "$BUILD_DIR"/conf
  mkdir -p "$BUILD_DIR"/downloads
  mkdir -p "$BUILD_DIR"/sstate-cache
  mkdir -p "$BUILD_DIR"/tmp
}
