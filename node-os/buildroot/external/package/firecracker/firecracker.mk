################################################################################
#
# firecracker
#
################################################################################

FIRECRACKER_VERSION = 1.11.0
FIRECRACKER_SOURCE = firecracker-v$(FIRECRACKER_VERSION)-x86_64.tgz
FIRECRACKER_SITE = https://github.com/firecracker-microvm/firecracker/releases/download/v$(FIRECRACKER_VERSION)
FIRECRACKER_LICENSE = Apache-2.0
FIRECRACKER_LICENSE_FILES = LICENSE

# Firecracker provides pre-built binaries
define FIRECRACKER_EXTRACT_CMDS
	$(TAR) -xzf $(FIRECRACKER_DL_DIR)/$(FIRECRACKER_SOURCE) -C $(@D)
endef

define FIRECRACKER_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/release-v$(FIRECRACKER_VERSION)-x86_64/firecracker-v$(FIRECRACKER_VERSION)-x86_64 \
		$(TARGET_DIR)/usr/bin/firecracker
	$(INSTALL) -D -m 0755 $(@D)/release-v$(FIRECRACKER_VERSION)-x86_64/jailer-v$(FIRECRACKER_VERSION)-x86_64 \
		$(TARGET_DIR)/usr/bin/jailer
endef

# Architecture-specific overrides
ifeq ($(BR2_aarch64),y)
FIRECRACKER_SOURCE = firecracker-v$(FIRECRACKER_VERSION)-aarch64.tgz
define FIRECRACKER_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/release-v$(FIRECRACKER_VERSION)-aarch64/firecracker-v$(FIRECRACKER_VERSION)-aarch64 \
		$(TARGET_DIR)/usr/bin/firecracker
	$(INSTALL) -D -m 0755 $(@D)/release-v$(FIRECRACKER_VERSION)-aarch64/jailer-v$(FIRECRACKER_VERSION)-aarch64 \
		$(TARGET_DIR)/usr/bin/jailer
endef
endif

$(eval $(generic-package))
