#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT/poky"
source oe-init-build-env "$BUILD_DIR"

bitbake graphene-node-image 2>&1 | tee "$BUILD_DIR/bitbake.log"
if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
  echo "BitBake build failed"
  tail -n 100 "$BUILD_DIR/bitbake.log"
  exit 1
fi
