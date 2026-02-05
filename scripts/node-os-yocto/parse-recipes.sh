#!/usr/bin/env bash
set -euo pipefail
set +u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT/poky"
source oe-init-build-env "$BUILD_DIR"
set -u

set -o pipefail
bitbake -p 2>&1 | tee "$BUILD_DIR/parse-recipes.log"
if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
  echo "Recipe parsing failed"
  tail -n 50 "$BUILD_DIR/parse-recipes.log"
  exit 1
fi

bitbake -e graphene-node-image > /dev/null
