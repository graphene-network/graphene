#!/bin/sh
# dm-verity Setup Script
# SPDX-License-Identifier: Apache-2.0
#
# Sets up dm-verity for the root filesystem.
# Called from initramfs before switching to real root.

set -e

# Expected root hash (embedded at build time)
EXPECTED_ROOT_HASH="${OPENCAPSULE_VERITY_ROOT:-}"

# Device paths
DATA_DEVICE="/dev/sda2"
HASH_DEVICE="/dev/sda3"
VERITY_NAME="vroot"

usage() {
    echo "Usage: $0 [verify|create]"
    echo "  verify - Verify and activate dm-verity"
    echo "  create - Create dm-verity hash (build-time only)"
    exit 1
}

verify_root() {
    if [ -z "$EXPECTED_ROOT_HASH" ]; then
        echo "ERROR: OPENCAPSULE_VERITY_ROOT not set"
        exit 1
    fi

    echo "Activating dm-verity for root filesystem..."
    echo "  Data device: $DATA_DEVICE"
    echo "  Hash device: $HASH_DEVICE"
    echo "  Expected hash: ${EXPECTED_ROOT_HASH:0:16}..."

    # Activate verity device
    veritysetup open "$DATA_DEVICE" "$VERITY_NAME" "$HASH_DEVICE" "$EXPECTED_ROOT_HASH"

    if [ $? -eq 0 ]; then
        echo "dm-verity activated successfully"
        echo "Root device: /dev/mapper/$VERITY_NAME"
    else
        echo "ERROR: dm-verity verification failed!"
        echo "Root filesystem may have been tampered with."
        exit 1
    fi
}

create_hash() {
    # Build-time only: create verity hash for image
    echo "Creating dm-verity hash for root filesystem..."

    OUTPUT=$(veritysetup format "$DATA_DEVICE" "$HASH_DEVICE")
    ROOT_HASH=$(echo "$OUTPUT" | grep "Root hash:" | awk '{print $3}')

    echo "Root hash: $ROOT_HASH"
    echo ""
    echo "Set this value in the build:"
    echo "  OPENCAPSULE_VERITY_ROOT=$ROOT_HASH"
}

case "${1:-verify}" in
    verify)
        verify_root
        ;;
    create)
        create_hash
        ;;
    *)
        usage
        ;;
esac
