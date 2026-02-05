#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

IMAGE=$(find "$BUILD_DIR/tmp/deploy/images" -name "graphene-node-image-*.ext4" | head -1)
if [[ -z "$IMAGE" ]]; then
  echo "ERROR: no image found"
  exit 1
fi

SIZE=$(stat -c%s "$IMAGE")
SIZE_MB=$((SIZE / 1024 / 1024))
echo "Image size: ${SIZE_MB}MB"
if [[ $SIZE_MB -gt 50 ]]; then
  echo "WARNING: image exceeds 50MB target"
else
  echo "PASS: image under 50MB"
fi
