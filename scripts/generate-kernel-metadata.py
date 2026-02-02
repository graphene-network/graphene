#!/usr/bin/env python3
"""
Generate kernel metadata JSON from built kernel binary.

Usage:
    python scripts/generate-kernel-metadata.py \
        --runtime python \
        --version 3.11 \
        --arch x86_64 \
        --kernel-path path/to/kernel.fc \
        --output python-3.11-x86_64.json
"""

import argparse
import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path

try:
    import toml
except ImportError:
    print("Error: toml package required. Install with: pip install toml", file=sys.stderr)
    sys.exit(1)


def calculate_blake3_hash(file_path: str) -> str:
    """Calculate BLAKE3 hash of a file."""
    try:
        import blake3
        hasher = blake3.blake3()
        with open(file_path, "rb") as f:
            for chunk in iter(lambda: f.read(8192), b""):
                hasher.update(chunk)
        return hasher.hexdigest()
    except ImportError:
        # Fallback to SHA256 if blake3 not available
        hasher = hashlib.sha256()
        with open(file_path, "rb") as f:
            for chunk in iter(lambda: f.read(8192), b""):
                hasher.update(chunk)
        return hasher.hexdigest()


def get_memory_config(matrix_path: str, runtime: str) -> tuple[int, int]:
    """Get memory configuration from matrix file."""
    try:
        with open(matrix_path) as f:
            config = toml.load(f)

        defaults = config.get("defaults", {})
        default_min = defaults.get("min_memory_mib", 128)
        default_rec = defaults.get("recommended_memory_mib", 256)

        runtime_config = config.get("runtimes", {}).get(runtime, {})
        min_mem = runtime_config.get("min_memory_mib", default_min)
        rec_mem = runtime_config.get("recommended_memory_mib", default_rec)

        return min_mem, rec_mem
    except Exception:
        return 128, 256


def generate_metadata(
    runtime: str,
    version: str,
    arch: str,
    kernel_path: str,
    matrix_path: str = "kernels/kernel-matrix.toml",
) -> dict:
    """Generate kernel metadata dictionary."""
    # Find the actual kernel file (might be a glob pattern)
    kernel_file = Path(kernel_path)
    if not kernel_file.exists():
        # Try to find it
        parent = kernel_file.parent
        pattern = kernel_file.name
        matches = list(parent.glob(pattern)) if parent.exists() else []
        if matches:
            kernel_file = matches[0]
        else:
            raise FileNotFoundError(f"Kernel file not found: {kernel_path}")

    binary_hash = calculate_blake3_hash(str(kernel_file))
    binary_size = kernel_file.stat().st_size

    # Get memory config from matrix
    min_mem, rec_mem = get_memory_config(matrix_path, runtime)

    # Get Unikraft version from matrix
    unikraft_version = "0.17.0"
    try:
        with open(matrix_path) as f:
            config = toml.load(f)
            unikraft_version = config.get("unikraft_version", unikraft_version)
    except Exception:
        pass

    return {
        "spec": {
            "runtime": runtime,
            "version": version,
            "arch": arch,
            "variant": None,
        },
        "binary_hash": binary_hash,
        "binary_size_bytes": binary_size,
        "min_memory_mib": min_mem,
        "recommended_memory_mib": rec_mem,
        "default_boot_args": "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules",
        "unikraft_version": unikraft_version,
        "built_at": datetime.now(timezone.utc).isoformat(),
    }


def main():
    parser = argparse.ArgumentParser(description="Generate kernel metadata")
    parser.add_argument("--runtime", required=True, help="Runtime name (python, node, etc)")
    parser.add_argument("--version", required=True, help="Runtime version")
    parser.add_argument("--arch", required=True, help="Architecture (x86_64, aarch64)")
    parser.add_argument("--kernel-path", required=True, help="Path to kernel binary")
    parser.add_argument("--matrix", default="kernels/kernel-matrix.toml", help="Path to matrix file")
    parser.add_argument("--output", required=True, help="Output JSON file path")
    args = parser.parse_args()

    metadata = generate_metadata(
        runtime=args.runtime,
        version=args.version,
        arch=args.arch,
        kernel_path=args.kernel_path,
        matrix_path=args.matrix,
    )

    with open(args.output, "w") as f:
        json.dump(metadata, f, indent=2)

    print(f"Generated metadata: {args.output}")


if __name__ == "__main__":
    main()
