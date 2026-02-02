# Unikernel Configurations

This directory contains Kraftfile configurations for building pre-built unikernels using [Unikraft](https://unikraft.org).

## Supported Runtimes

Kernels are built from the [Unikraft Application Catalog](https://github.com/unikraft/catalog). Only versions available in the catalog are supported.

| Runtime | Versions | Directory |
|---------|----------|-----------|
| Python  | 3.10, 3.12 | `python/3.10/`, `python/3.12/` |
| Node.js | 20, 21 | `node/20/`, `node/21/` |
| Bun     | 1.1 | `bun/1.1/` |

## Directory Structure

```
kernels/
├── kernel-matrix.toml      # Build matrix configuration
├── bun/
│   └── 1.1/
│       └── Kraftfile.yaml
├── node/
│   ├── 20/
│   │   └── Kraftfile.yaml
│   └── 21/
│       └── Kraftfile.yaml
└── python/
    ├── 3.10/
    │   └── Kraftfile.yaml
    └── 3.12/
        └── Kraftfile.yaml
```

## Kraftfile Format

Each runtime version has a `Kraftfile.yaml` that uses the modern runtime directive:

```yaml
spec: v0.6

# Pull pre-built runtime from Unikraft catalog
runtime: unikraft.org/python:3.12

# Target Firecracker hypervisor on x86_64
targets:
  - platform: fc
    architecture: x86_64

# Default entry point
cmd: ["/usr/bin/python3", "/app/main.py"]
```

The `runtime:` directive pulls pre-built images from `unikraft.org`, which is the recommended approach. This replaces the older `libraries:` approach which required specific package versions to exist in kraft's package index.

## Version Matrix

The `kernel-matrix.toml` defines all runtime/version combinations to build:

```toml
unikraft_version = "0.17.0"

[defaults]
min_memory_mib = 128
recommended_memory_mib = 256
boot_args = "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules"

[runtimes.python]
versions = ["3.10", "3.12"]
architectures = ["x86_64"]

[runtimes.node]
versions = ["20", "21"]
min_memory_mib = 256
recommended_memory_mib = 512

[runtimes.bun]
versions = ["1.1"]
min_memory_mib = 256
recommended_memory_mib = 512
```

## CI/CD

Kernels are built automatically by GitHub Actions when:
- Any file in `kernels/` changes
- The workflow file `.github/workflows/kernel-build.yml` changes
- Manually triggered via workflow_dispatch

The build workflow:
1. Parses `kernel-matrix.toml` to generate the build matrix
2. Installs [KraftKit](https://github.com/unikraft/kraftkit) from GitHub releases
3. Runs `kraft build --plat fc --arch x86_64` for each configuration
4. Uploads artifacts to GitHub Releases

## Adding a New Runtime Version

1. **Check Unikraft Catalog availability**
   ```bash
   # Browse https://github.com/unikraft/catalog for available runtimes
   ```

2. **Update the version matrix**
   ```toml
   # In kernel-matrix.toml
   [runtimes.python]
   versions = ["3.10", "3.12", "3.13"]  # Add new version
   ```

3. **Create the Kraftfile**
   ```bash
   mkdir -p kernels/python/3.13
   ```

   ```yaml
   # kernels/python/3.13/Kraftfile.yaml
   spec: v0.6
   runtime: unikraft.org/python:3.13
   targets:
     - platform: fc
       architecture: x86_64
   cmd: ["/usr/bin/python3", "/app/main.py"]
   ```

4. **Push to trigger CI build**

## Version Constraints

Not all runtime versions are available in the Unikraft catalog. Before adding a version, verify it exists:

- **Python**: 3.10, 3.12, 3.13 available (3.11 NOT available)
- **Node.js**: 18, 20, 21 available (22 NOT available)
- **Bun**: 1.1 available
- **Deno**: NOT available in catalog

Check the [Unikraft Catalog](https://github.com/unikraft/catalog) for the current list.

## Local Development

To build a kernel locally:

```bash
# Install kraft CLI
curl --proto '=https' --tlsv1.2 -sSf https://get.kraftkit.sh | sh

# Build a specific kernel
cd kernels/python/3.12
kraft build --plat fc --arch x86_64

# Output will be in .unikraft/build/
```

## Troubleshooting

**"could not find: lib/xxx:version"**
- The library version doesn't exist in kraft's package index
- Solution: Use the `runtime:` directive instead of `libraries:`

**"could not determine how to build initrd from: ./rootfs"**
- The Kraftfile references a rootfs directory that doesn't exist
- Solution: Remove `rootfs:` line or create the directory

**kraft installation fails in CI**
- The installation script may have changed
- Solution: Install from GitHub releases instead of the convenience script
