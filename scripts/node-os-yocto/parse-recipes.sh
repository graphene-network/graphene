#!/usr/bin/env bash
set -euo pipefail
set +u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
set +u

cd "$YOCTO_TOP"
export BBSERVER="${BBSERVER:-}"
export ZSH_NAME="${ZSH_NAME:-}"
if [[ -n "${YOCTO_TEMPLATECONF}" ]]; then
  TEMPLATECONF="${YOCTO_TEMPLATECONF}" source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
else
  source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
fi
set -u

set -o pipefail
bitbake -p 2>&1 | tee "$BUILD_DIR/parse-recipes.log"
if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
  echo "Recipe parsing failed"
  tail -n 50 "$BUILD_DIR/parse-recipes.log"
  exit 1
fi

bitbake -e graphene-node-image > /dev/null
