# OpenCapsule OS Security Hardening
# SPDX-License-Identifier: Apache-2.0
#
# Security hardening settings for OpenCapsule Node OS builds.

# Compiler hardening flags
SECURITY_CFLAGS = "-fstack-protector-strong -D_FORTIFY_SOURCE=2"
SECURITY_LDFLAGS = "-Wl,-z,relro,-z,now"

TARGET_CFLAGS:append = " ${SECURITY_CFLAGS}"
TARGET_LDFLAGS:append = " ${SECURITY_LDFLAGS}"

# Strip binaries for smaller size
INHIBIT_PACKAGE_STRIP = "0"

# Remove debug symbols
INHIBIT_PACKAGE_DEBUG_SPLIT = "1"

# No static libraries
PACKAGE_EXCLUDE_COMPLEMENTARY = ".*-staticdev"

# Kernel hardening config
# Note: security.cfg is added via SRC_URI in linux-yocto bbappend, not via KERNEL_FEATURES

# Remove unnecessary kernel features
KERNEL_FEATURES:remove = "features/sound/snd_hda_intel.scc"

# Disable core dumps
CORE_IMAGE_EXTRA_INSTALL:remove = "gdb strace"

# Audit and logging
# TODO(#113): Add audit support when meta-oe layer is available
# IMAGE_INSTALL:append = " audit"

# Package exclusions - things that should never be in production
PACKAGE_EXCLUDE = " \
    gdb \
    strace \
    ltrace \
    valgrind \
    tcpdump \
    nc \
    netcat \
    telnet \
    openssh-sftp-server \
"

# Remove documentation
DISTRO_FEATURES:remove = "doc"

# No X11 or Wayland
DISTRO_FEATURES:remove = "x11 wayland opengl"

# Minimal systemd configuration
DISTRO_FEATURES:append = " systemd"
DISTRO_FEATURES:remove = "sysvinit"
VIRTUAL-RUNTIME_init_manager = "systemd"

# Security-focused distro features
DISTRO_FEATURES:append = " seccomp"
