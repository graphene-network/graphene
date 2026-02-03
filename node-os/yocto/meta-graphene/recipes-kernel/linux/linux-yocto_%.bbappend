# Graphene Node kernel customization
# SPDX-License-Identifier: Apache-2.0

FILESEXTRAPATHS:prepend := "${THISDIR}/files:"

# Machine compatibility - required for linux-yocto to build for these machines
COMPATIBLE_MACHINE:graphene-node-x86_64 = "graphene-node-x86_64"
COMPATIBLE_MACHINE:graphene-node-aarch64 = "graphene-node-aarch64"

# Map our custom machine to an existing BSP for kernel metadata
# This avoids needing to create full BSP definitions in yocto-kernel-cache
KMACHINE:graphene-node-x86_64 = "intel-corei7-64"
KMACHINE:graphene-node-aarch64 = "qemuarm64"

# Use standard kernel branch
KBRANCH:graphene-node-x86_64 = "v5.15/standard/intel"
KBRANCH:graphene-node-aarch64 = "v5.15/standard/arm-versatile-926ejs"

# Add our defconfig and configuration fragments
SRC_URI:append = " \
    file://defconfig \
    file://cfg/security.scc \
    file://cfg/security.cfg \
"

# Apply security configuration as kernel features
KERNEL_FEATURES:append = " cfg/security.scc"

# Ensure our defconfig is used (merged with kernel metadata config)
KERNEL_DEFCONFIG:graphene-node-x86_64 = "defconfig"
KERNEL_DEFCONFIG:graphene-node-aarch64 = "defconfig"
