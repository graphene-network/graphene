# Yocto Node OS Build Helpers

## Purpose
These scripts mirror the key steps of `.github/workflows/node-os-yocto.yml` but are designed for local use. They allow you to install dependencies, prepare the Yocto build environment, run BitBake, and verify the resulting image without needing GitHub Actions.

## Scripts overview
- `scripts/node-os-yocto/env.sh`: load `node-os/os-matrix.toml`, export common variables like `YOCTO_RELEASE`, `BUILD_DIR`, and provide helpers such as `ensure_build_dirs`.
- `install-deps.sh` & `enable-userns.sh`: install required Ubuntu packages and relax kernel restrictions for pseudo operations.
- `clone-deps.sh`: fetch `poky` and `meta-rust-bin` sources next to the repository.
- `configure-build.sh`: initializes `conf/bblayers.conf` and `conf/local.conf` for a given machine, pointing BitBake at the checked-out repo (`EXTERNALSRC`).
- `parse-recipes.sh`: runs `bitbake -p` and validates the `graphene-node-image` recipe.
- `build-image.sh`: executes `bitbake graphene-node-image` and logs output for troubleshooting.
- Post-build scripts: `verify-shell-removal.sh`, `check-image-size.sh`, and `generate-build-metadata.sh` (writes `build-info.json`).

## How to use
```bash
# Source shared config (overridable via env vars)
source scripts/node-os-yocto/env.sh

# Install system dependencies (once per machine)
sudo scripts/node-os-yocto/install-deps.sh
sudo scripts/node-os-yocto/enable-userns.sh

# Clone Yocto dependencies
scripts/node-os-yocto/clone-deps.sh

# Configure the build for your target (default machine: graphene-node-x86_64)
scripts/node-os-yocto/configure-build.sh graphene-node-x86_64

# Build the graphene-worker binary that Yocto installs (externalsrc expects it in target/<rust-target>/release)
# For x86_64 builds:
cargo build --bin graphene-worker --release --target x86_64-unknown-linux-gnu

# Optionally validate the recipes
scripts/node-os-yocto/parse-recipes.sh

# Run the actual Yocto build (this takes time)
scripts/node-os-yocto/build-image.sh

# Verify artifacts
scripts/node-os-yocto/verify-shell-removal.sh
scripts/node-os-yocto/check-image-size.sh
scripts/node-os-yocto/generate-build-metadata.sh graphene-node-x86_64
```

## Troubleshooting
If you see `do_install` errors like `cannot stat .../target/x86_64-unknown-linux-gnu/release/graphene-worker`, the worker binary is missing from the externalsrc build directory. Rebuild it (command above) or force the Yocto recipe to recompile with `bitbake -c compile -f graphene-node`.

## Re-running for different machines
Repeat the configure/build steps with another machine name. Existing downloads and sstate cache live under `$BUILD_DIR`, so they are shared, but you may want to clean (`rm -rf $BUILD_DIR/tmp`) if you change major settings.
