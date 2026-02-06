# Kernel Library

Pre-built unikernel management for the Graphene Network.

## Overview

This module provides a registry for managing pre-built unikernels for common runtimes (Python, Node.js, Bun). Kernels are built once via CI using the [Unikraft](https://unikraft.org) framework and downloaded on-demand by worker nodes.

## Supported Runtimes

Kernels are built from the [Unikraft Application Catalog](https://github.com/unikraft/catalog). Only versions available in the catalog are supported:

| Runtime | Versions | Memory |
|---------|----------|--------|
| Python  | 3.10, 3.12 | 128-256 MiB |
| Node.js | 20, 21 | 256-512 MiB |
| Bun     | 1.1 | 256-512 MiB |

**Note**: Version availability depends on the Unikraft catalog. Check `kernels/kernel-matrix.toml` for the current supported versions.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     KernelRegistry Trait                    │
├─────────────────────────────────────────────────────────────┤
│  resolve("python-3.12") → KernelSpec                        │
│  get(spec) → Option<PathBuf>  (check cache)                 │
│  ensure(spec) → PathBuf       (download if needed)          │
│  list_available() → Vec<KernelSpec>                         │
│  get_metadata(spec) → KernelMetadata                        │
└─────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────────┐    ┌─────────────────────┐
│ LocalKernelRegistry │    │  MockKernelRegistry │
│   (production)      │    │     (testing)       │
└─────────────────────┘    └─────────────────────┘
```

## Usage

```rust
use graphene_node::kernel::{KernelRegistry, LocalKernelRegistry, KernelSpec};
use graphene_node::kernel::matrix::KernelMatrix;

// Load version matrix
let matrix = KernelMatrix::from_file("kernels/kernel-matrix.toml")?;

// Create registry
let registry = LocalKernelRegistry::new(matrix)?;

// Resolve and ensure kernel is available
let spec = registry.resolve("python-3.12")?;
let kernel_path = registry.ensure(&spec).await?;

// Get metadata for Firecracker configuration
let metadata = registry.get_metadata(&spec)?;
println!("Memory: {} MiB", metadata.recommended_memory_mib);
println!("Boot args: {}", metadata.boot_args());
```

## Module Structure

| File | Description |
|------|-------------|
| `mod.rs` | `KernelRegistry` trait and `KernelError` types |
| `types.rs` | `KernelSpec`, `KernelMetadata`, `Runtime`, `Architecture` |
| `local.rs` | `LocalKernelRegistry` - filesystem-based implementation |
| `matrix.rs` | TOML parser for `kernel-matrix.toml` |
| `mock.rs` | `MockKernelRegistry` - configurable mock for testing |

## Storage Layout

Kernels are stored under `~/.graphene/kernels/`:

```
~/.graphene/kernels/
├── blobs/
│   └── <blake3-hash>           # Actual kernel binaries
├── refs/
│   └── python-3.12-x86_64      # Symlinks to blobs
└── metadata/
    └── python-3.12-x86_64.json # Kernel metadata
```

## Testing

The `MockKernelRegistry` supports configurable behaviors for comprehensive testing:

```rust
use graphene_node::kernel::mock::{MockKernelRegistry, MockBehavior};

// Test happy path
let registry = MockKernelRegistry::new();

// Test network failures
let registry = MockKernelRegistry::with_behavior(MockBehavior::DownloadFailure);

// Test corrupted downloads
let registry = MockKernelRegistry::with_behavior(MockBehavior::CorruptedKernel);

// Pre-cache kernels for faster tests
let mut registry = MockKernelRegistry::new();
registry.pre_cache(&spec, PathBuf::from("/path/to/kernel"));
```

## Version Matrix

The `kernels/kernel-matrix.toml` file defines which runtimes and versions to build:

```toml
unikraft_version = "0.17.0"

[defaults]
min_memory_mib = 128
recommended_memory_mib = 256

[runtimes.python]
# Available in catalog: 3.10, 3.12, 3.13
versions = ["3.10", "3.12"]
architectures = ["x86_64"]

[runtimes.node]
# Available in catalog: 18, 20, 21
versions = ["20", "21"]
min_memory_mib = 256
recommended_memory_mib = 512

[runtimes.bun]
# Available in catalog: 1.1
versions = ["1.1"]
min_memory_mib = 256
recommended_memory_mib = 512
```

## CI Pipeline

Kernels are built automatically via GitHub Actions (`.github/workflows/kernel-build.yml`) when Kraftfiles change. The workflow:

1. Generates a build matrix from `kernel-matrix.toml`
2. Builds each runtime/version combination using `kraft build`
3. Uploads kernel artifacts to GitHub Releases

Built kernels are downloaded by worker nodes on first use and cached locally.

## Adding New Runtimes

To add a new runtime or version:

1. Check the [Unikraft Catalog](https://github.com/unikraft/catalog) for availability
2. Add entry to `kernels/kernel-matrix.toml`
3. Create `kernels/<runtime>/<version>/Kraftfile.yaml`:
   ```yaml
   spec: v0.6
   runtime: unikraft.org/<runtime>:<version>
   targets:
     - platform: fc
       architecture: x86_64
   cmd: ["/path/to/entrypoint"]
   ```
4. Push changes to trigger CI build
