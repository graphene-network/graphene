# Unikraft Node.js Example

A simple Express.js application demonstrating unikernel builds with Unikraft.

## Prerequisites

- [kraft CLI](https://unikraft.org/docs/cli) installed
- Docker (for rootfs building)

## Manual Build

```bash
# Build the unikernel
kraft build --plat fc --arch x86_64

# Run with Firecracker (requires Linux)
kraft run --plat fc --arch x86_64
```

## What This Example Demonstrates

1. **Dockerfile validation** - The Dockerfile uses only allowed commands
2. **Kraftfile generation** - Shows the expected Kraftfile format
3. **Express.js runtime** - Node.js 20 running as a unikernel
4. **Health endpoint** - `/health` endpoint for liveness checks

## Endpoints

- `GET /` - Returns a greeting message
- `GET /health` - Returns health status
