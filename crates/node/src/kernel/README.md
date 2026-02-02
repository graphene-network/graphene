# Kernel Library

Pre-built unikernel management for the Graphene Network.

## Overview

This module provides a registry for managing pre-built unikernels for common runtimes (Python, Node.js, Bun, Deno). Kernels are built once via CI and downloaded on-demand by worker nodes.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     KernelRegistry Trait                    │
├─────────────────────────────────────────────────────────────┤
│  resolve("python-3.11") → KernelSpec                        │
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
use monad_node::kernel::{KernelRegistry, LocalKernelRegistry, KernelSpec};
use monad_node::kernel::matrix::KernelMatrix;

// Load version matrix
let matrix = KernelMatrix::from_file("kernels/kernel-matrix.toml")?;

// Create registry
let registry = LocalKernelRegistry::new(matrix)?;

// Resolve and ensure kernel is available
let spec = registry.resolve("python-3.11")?;
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
│   └── python-3.11-x86_64      # Symlinks to blobs
└── metadata/
    └── python-3.11-x86_64.json # Kernel metadata
```

## Testing

The `MockKernelRegistry` supports configurable behaviors for comprehensive testing:

```rust
use monad_node::kernel::mock::{MockKernelRegistry, MockBehavior};

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
versions = ["3.11", "3.12"]
architectures = ["x86_64"]

[runtimes.node]
versions = ["20", "22"]
min_memory_mib = 256
recommended_memory_mib = 512
```

## CI Pipeline

Kernels are built automatically via GitHub Actions when Kraftfiles change. Built kernels are uploaded to GitHub Releases for download by worker nodes.
