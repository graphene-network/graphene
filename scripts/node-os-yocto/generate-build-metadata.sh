#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

MACHINE="${1:-graphene-node-x86_64}"

IMAGE=$(find "$BUILD_DIR/tmp/deploy/images" -name "graphene-node-image-*.ext4" | head -1)
if [[ -z "$IMAGE" ]]; then
  echo "ERROR: no image found"
  exit 1
fi

cat > "$BUILD_DIR/build-info.json" <<METADATA
{
  "yocto_release": "${YOCTO_RELEASE}",
  "machine": "${MACHINE}",
  "image_size_bytes": $(stat -c%s "$IMAGE"),
  "build_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "commit_sha": "$(git -C "$REPO_ROOT" rev-parse HEAD)"
}
METADATA
