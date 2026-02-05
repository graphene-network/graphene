#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

IMAGE=$(find "$BUILD_DIR/tmp/deploy/images" -name "graphene-node-image-*.ext4" | head -1)
if [[ -z "${IMAGE}" ]]; then
  echo "ERROR: no image found"
  exit 1
fi

TMP_ROOTFS="/tmp/graphene-node-rootfs"
sudo mkdir -p "$TMP_ROOTFS"
sudo mount -o loop,ro "$IMAGE" "$TMP_ROOTFS"

SHELL_FOUND=0
for shell in sh bash ash dash zsh; do
  if sudo test -f "$TMP_ROOTFS/bin/$shell" || sudo test -f "$TMP_ROOTFS/usr/bin/$shell"; then
    echo "Shell found: $shell"
    SHELL_FOUND=1
  fi
done

sudo umount "$TMP_ROOTFS"
sudo rmdir "$TMP_ROOTFS"

if [[ "$SHELL_FOUND" -ne 0 ]]; then
  echo "FAIL: Shell binaries present"
  exit 1
fi

echo "PASS: No shells in rootfs"
