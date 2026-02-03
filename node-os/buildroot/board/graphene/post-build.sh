#!/bin/bash
# Graphene Node OS post-build script
# Removes shells and hardens the rootfs

set -e

TARGET_DIR="${1}"

echo "=== Graphene Node OS Post-Build ==="
echo "Target directory: ${TARGET_DIR}"

# Remove all shell binaries (critical security measure)
echo "Removing shell binaries..."
rm -f "${TARGET_DIR}/bin/sh" \
      "${TARGET_DIR}/bin/bash" \
      "${TARGET_DIR}/bin/ash" \
      "${TARGET_DIR}/bin/hush" \
      "${TARGET_DIR}/bin/cttyhack" \
      "${TARGET_DIR}/usr/bin/sh" \
      "${TARGET_DIR}/usr/bin/bash" \
      2>/dev/null || true

# Update /etc/passwd to use /sbin/nologin for all users
if [ -f "${TARGET_DIR}/etc/passwd" ]; then
    echo "Updating user shells to nologin..."
    sed -i 's|:/bin/sh$|:/sbin/nologin|g' "${TARGET_DIR}/etc/passwd"
    sed -i 's|:/bin/bash$|:/sbin/nologin|g' "${TARGET_DIR}/etc/passwd"
    sed -i 's|:/bin/ash$|:/sbin/nologin|g' "${TARGET_DIR}/etc/passwd"
fi

# Remove unnecessary files
echo "Removing unnecessary files..."
rm -rf "${TARGET_DIR}/usr/share/man" \
       "${TARGET_DIR}/usr/share/doc" \
       "${TARGET_DIR}/usr/share/info" \
       "${TARGET_DIR}/usr/share/locale" \
       "${TARGET_DIR}/var/cache" \
       2>/dev/null || true

# Create /var/cache for runtime
mkdir -p "${TARGET_DIR}/var/cache"

# Remove getty and login utilities (no interactive access)
rm -f "${TARGET_DIR}/sbin/getty" \
      "${TARGET_DIR}/bin/login" \
      "${TARGET_DIR}/usr/bin/passwd" \
      2>/dev/null || true

# Set restrictive permissions on sensitive directories
if [ -d "${TARGET_DIR}/etc" ]; then
    chmod 755 "${TARGET_DIR}/etc"
fi

# Create graphene-specific directories
mkdir -p "${TARGET_DIR}/etc/graphene"
mkdir -p "${TARGET_DIR}/var/lib/graphene"
mkdir -p "${TARGET_DIR}/var/lib/firecracker"
mkdir -p "${TARGET_DIR}/var/log/graphene"

# Create minimal /etc/os-release
cat > "${TARGET_DIR}/etc/os-release" << 'EOF'
NAME="Graphene Node OS"
ID=graphene
VERSION_ID="0.1.0"
PRETTY_NAME="Graphene Node OS 0.1.0"
HOME_URL="https://graphene.network"
EOF

# Create /etc/hosts
cat > "${TARGET_DIR}/etc/hosts" << 'EOF'
127.0.0.1	localhost
::1		localhost
EOF

# Verify no shells exist
echo "Verifying shell removal..."
for shell in sh bash ash hush zsh; do
    if [ -f "${TARGET_DIR}/bin/${shell}" ] || [ -f "${TARGET_DIR}/usr/bin/${shell}" ]; then
        echo "ERROR: Shell binary found: ${shell}"
        exit 1
    fi
done

echo "=== Post-build complete ==="
echo "Shell binaries removed successfully"
