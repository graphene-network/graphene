#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT/poky"
export BBSERVER="${BBSERVER:-}"
export ZSH_NAME="${ZSH_NAME:-}"
set +u
source oe-init-build-env "$BUILD_DIR"
set -u

set +e
bitbake graphene-node-image 2>&1 | tee "$BUILD_DIR/bitbake.log"
bb_status=${PIPESTATUS[0]}
tee_status=${PIPESTATUS[1]}
set -e

if [[ $bb_status -ne 0 ]]; then
  echo "BitBake build failed (exit ${bb_status})"
  tail -n 100 "$BUILD_DIR/bitbake.log"
  exit "$bb_status"
fi

if [[ $tee_status -ne 0 && $tee_status -ne 141 ]]; then
  echo "WARNING: tee failed (exit ${tee_status}); build succeeded."
fi
