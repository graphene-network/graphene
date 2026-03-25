# dm-verity Setup Scripts
# SPDX-License-Identifier: Apache-2.0
#
# Provides scripts for dm-verity root filesystem verification.
# The root hash is embedded at build time and verified at boot.

SUMMARY = "dm-verity setup for OpenCapsule Node OS"
LICENSE = "Apache-2.0"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/Apache-2.0;md5=89aea4e17d99a7cacdbeed46a0096b10"

SRC_URI = " \
    file://verity-setup.sh \
    file://verity.mount \
"

inherit allarch

do_install() {
    install -d ${D}${sbindir}
    install -m 0755 ${WORKDIR}/verity-setup.sh ${D}${sbindir}/verity-setup

    # Install systemd mount unit for verity device
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${WORKDIR}/verity.mount ${D}${systemd_system_unitdir}/
}

RDEPENDS:${PN} = "cryptsetup"

inherit systemd
SYSTEMD_SERVICE:${PN} = "verity.mount"
