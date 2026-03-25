# OpenCapsule Node kernel customization
# SPDX-License-Identifier: Apache-2.0

FILESEXTRAPATHS:prepend := "${THISDIR}/files:"

# Machine compatibility - required for linux-yocto to build for these machines
COMPATIBLE_MACHINE:opencapsule-node-x86_64 = "opencapsule-node-x86_64"
COMPATIBLE_MACHINE:opencapsule-node-aarch64 = "opencapsule-node-aarch64"

# Map our custom machine to an existing BSP for kernel metadata
# This avoids needing to create full BSP definitions in yocto-kernel-cache
KMACHINE:opencapsule-node-x86_64 = "intel-corei7-64"
KMACHINE:opencapsule-node-aarch64 = "qemuarm64"

# Kernel branch - set via OPENCAPSULE_KERNEL_BRANCH in local.conf or use default
# Default matches Whinlatter's linux-yocto 6.16 (see node-os/os-matrix.toml)
OPENCAPSULE_KERNEL_BRANCH ?= "v6.16/standard/base"
KBRANCH:opencapsule-node-x86_64 = "${OPENCAPSULE_KERNEL_BRANCH}"
KBRANCH:opencapsule-node-aarch64 = "${OPENCAPSULE_KERNEL_BRANCH}"

# Add our defconfig and configuration fragments
SRC_URI:append = " \
    file://defconfig \
    file://security.cfg \
"

# Ensure our defconfig is used (merged with kernel metadata config)
KERNEL_DEFCONFIG:opencapsule-node-x86_64 = "defconfig"
KERNEL_DEFCONFIG:opencapsule-node-aarch64 = "defconfig"
