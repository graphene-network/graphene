#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

export REPO_ROOT
export SSTATE_BUCKET="${SSTATE_BUCKET:-graphene-sstate}"
export BUILD_DIR="${BUILD_DIR:-/tmp/yocto-build}"
export NODE_OS_MATRIX="${NODE_OS_MATRIX:-$REPO_ROOT/node-os/os-matrix.toml}"

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

function ensure_build_dirs() {
  mkdir -p "$BUILD_DIR"/conf
  mkdir -p "$BUILD_DIR"/downloads
  mkdir -p "$BUILD_DIR"/sstate-cache
  mkdir -p "$BUILD_DIR"/tmp
}
