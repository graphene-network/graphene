################################################################################
#
# graphene-node
#
################################################################################

GRAPHENE_NODE_VERSION = 0.1.0
GRAPHENE_NODE_SITE = $(call github,marcus-sa,monad,main)
GRAPHENE_NODE_LICENSE = Apache-2.0
GRAPHENE_NODE_LICENSE_FILES = LICENSE

# Build from local source during development
GRAPHENE_NODE_SITE_METHOD = local
GRAPHENE_NODE_SITE = $(realpath $(BR2_EXTERNAL_GRAPHENE_PATH)/../..)

GRAPHENE_NODE_CARGO_ENV = \
	CARGO_HOME=$(HOST_DIR)/share/cargo

# Environment variables for attestation values (set by CI)
GRAPHENE_NODE_CARGO_BUILD_ARGS = --release --package monad_node --bin graphene-node

# We need Rust toolchain
GRAPHENE_NODE_DEPENDENCIES = host-rustc

define GRAPHENE_NODE_BUILD_CMDS
	cd $(@D) && \
	$(TARGET_MAKE_ENV) $(GRAPHENE_NODE_CARGO_ENV) \
	cargo build $(GRAPHENE_NODE_CARGO_BUILD_ARGS)
endef

define GRAPHENE_NODE_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/release/graphene-node \
		$(TARGET_DIR)/usr/bin/graphene-node

	# Install default config if not exists
	if [ ! -f $(TARGET_DIR)/etc/graphene/node-config.toml ]; then \
		$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_GRAPHENE_PATH)/board/graphene/node-config.toml \
			$(TARGET_DIR)/etc/graphene/node-config.toml; \
	fi
endef

$(eval $(generic-package))
