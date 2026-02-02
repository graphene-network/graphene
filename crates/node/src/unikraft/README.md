# Unikraft Build Pipeline

This module implements the Unikraft build pipeline for compiling Dockerfiles into minimal unikernel images.

## Overview

The build pipeline takes a Dockerfile + source code bundle and produces a sealed `.unik` binary that can be executed by Firecracker. This is fundamentally different from container builds - unikernels have no shell, no package manager at runtime, and run as a single process.

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   BuildJob      │────▶│  KraftBuilder    │────▶│ UnikernelImage  │
│  - Dockerfile   │     │  - validate      │     │  - blake3 hash  │
│  - source.tar   │     │  - generate      │     │  - path         │
│  - manifest     │     │  - build (kraft) │     │  - size         │
└─────────────────┘     └──────────────────┘     └─────────────────┘
```

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | `UnikernelBuilder` trait + `UnikraftError` enum |
| `types.rs` | Core types: `BuildJob`, `UnikernelImage`, `Kraftfile`, `Runtime` |
| `dockerfile.rs` | `DockerfileParser` + `DockerfileValidator` |
| `kraft.rs` | `KraftBuilder` - real implementation using kraft CLI |
| `mock.rs` | `MockKraftBuilder` - test double with spy state |

## Usage

### Building a Unikernel

```rust
use monad_node::unikraft::{
    BuildJob, BuildManifest, KraftBuilder, KraftConfig,
    ResourceLimits, Runtime, UnikernelBuilder,
};

// Create builder with custom config
let config = KraftConfig {
    kraft_bin: PathBuf::from("/usr/local/bin/kraft"),
    cache_dir: PathBuf::from("/var/cache/graphene/unikraft"),
    build_timeout: Duration::from_secs(300),
};
let builder = KraftBuilder::new(config);

// Define the build job
let job = BuildJob::new(
    "job-123",
    dockerfile_content,
    source_tar_bytes,
    BuildManifest {
        runtime: Runtime::Node20,
        entrypoint: vec!["node".into(), "index.js".into()],
        resources: ResourceLimits::default(),
    },
);

// Build the unikernel
let image = builder.build(&job).await?;
println!("Built: {:?} ({} bytes)", image.path, image.size_bytes);
```

### Validating a Dockerfile

```rust
let builder = KraftBuilder::with_defaults();
let validated = builder.validate_dockerfile(dockerfile)?;
println!("Runtime: {:?}", validated.runtime);
println!("Entrypoint: {:?}", validated.entrypoint);
```

### Testing with Mock

```rust
use monad_node::unikraft::{MockKraftBuilder, MockBuildBehavior};

// Happy path
let builder = MockKraftBuilder::happy_path();

// Simulate failures
let builder = MockKraftBuilder::failure(1, "kraft: command not found");
let builder = MockKraftBuilder::timeout(Duration::from_secs(5));
let builder = MockKraftBuilder::validation_error("Invalid Dockerfile");

// Check spy state
assert_eq!(builder.build_count(), 1);
assert!(builder.was_job_built("job-123"));
```

## Dockerfile Validation

### Allowed Commands

| Command | Notes |
|---------|-------|
| `FROM` | Must be `graphene/node:20` |
| `COPY` | Copy files into image |
| `WORKDIR` | Set working directory |
| `ENV` | Set environment variables |
| `CMD` | **Exec form only**: `["node", "index.js"]` |
| `ENTRYPOINT` | **Exec form only** |
| `ARG` | Build arguments |
| `RUN` | **Restricted**: `npm install`, `npm ci`, `yarn install` only |
| `LABEL` | Metadata labels |

### Forbidden Commands

| Command | Reason |
|---------|--------|
| `USER` | Unikernels have no user system |
| `VOLUME` | No dynamic volume mounting |
| `SHELL` | No shell exists in unikernels |
| `ADD` | Use COPY instead |
| `EXPOSE` | Network configured externally |
| `HEALTHCHECK` | No container orchestration |
| `STOPSIGNAL` | Single process, no signal handling |

### Shell Form Rejection

```dockerfile
# REJECTED - shell form requires /bin/sh
CMD node index.js

# ACCEPTED - exec form
CMD ["node", "index.js"]
```

## Kraftfile Generation

The builder generates a Kraftfile from the manifest:

```yaml
spec: v0.6
name: job-123
runtime: node:20
rootfs: ./Dockerfile
cmd: ["node", "index.js"]
```

## Error Handling

```rust
match builder.build(&job).await {
    Ok(image) => println!("Success: {}", hex::encode(image.hash)),
    Err(UnikraftError::DockerfileParseError(msg)) => eprintln!("Parse error: {}", msg),
    Err(UnikraftError::UnsupportedCommand { command, reason }) => {
        eprintln!("Forbidden command '{}': {}", command, reason)
    }
    Err(UnikraftError::UnsupportedBaseImage(img)) => {
        eprintln!("Use graphene/node:20, not {}", img)
    }
    Err(UnikraftError::BuildTimeout { elapsed, limit }) => {
        eprintln!("Build timed out after {:?}", elapsed)
    }
    Err(UnikraftError::BuildFailed { exit_code, stderr }) => {
        eprintln!("kraft failed ({}): {}", exit_code, stderr)
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

## Supported Runtimes

| Runtime | Base Image | Status |
|---------|------------|--------|
| Node.js 20 | `graphene/node:20` | Supported |
| Python 3.12 | `graphene/python:3.12` | Planned |

## Testing

```bash
# Unit tests
cargo test -p monad_node --lib

# E2E tests (requires kraft CLI on Linux)
cargo test -p monad_node --features e2e -- --ignored
```
