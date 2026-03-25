# Yocto Build Helpers

These scripts mirror the key steps of `.github/workflows/node-os-yocto.yml`, but run locally so you can reproduce the build job on your own machine.

## Shared setup
1. `source scripts/node-os-yocto/env.sh` (or run `./scripts/node-os-yocto/load-config.sh`) to export configuration variables such as `YOCTO_RELEASE` and `BUILD_DIR`. You can override defaults with environment variables like `BUILD_DIR=/tmp/custom-yocto`.
2. `clone-deps.sh` will fetch the required Yocto layers plus `meta-rust-bin`. For Scarthgap it uses `$REPO_ROOT/poky`; for Whinlatter it uses `$REPO_ROOT/yocto/layers`.
3. `ensure_build_dirs` is invoked by `configure-build.sh`, but you can also call it manually (`source .../env.sh && ensure_build_dirs`).

## Common workflows
```bash
# Install prerequisites and prep the environment once
sudo scripts/node-os-yocto/install-deps.sh
sudo scripts/node-os-yocto/enable-userns.sh
scripts/node-os-yocto/clone-deps.sh

# Configure the Yocto build (pass MACHINE if you need aarch64)
scripts/node-os-yocto/configure-build.sh opencapsule-node-x86_64

# Optional quick validation
scripts/node-os-yocto/parse-recipes.sh

# Full build
scripts/node-os-yocto/build-image.sh

# Post-build checks
scripts/node-os-yocto/verify-shell-removal.sh
scripts/node-os-yocto/check-image-size.sh
scripts/node-os-yocto/generate-build-metadata.sh opencapsule-node-x86_64
```

Each script is idempotent; rerun them and they will skip cloning steps when repositories already exist.

When you need to change the machine, rerun `configure-build.sh` before building.
