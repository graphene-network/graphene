#!/usr/bin/env python3
"""
Generate GitHub Actions build matrix from kernel-matrix.toml.

Usage:
    python scripts/generate-build-matrix.py --matrix kernels/kernel-matrix.toml

Outputs JSON matrix for GitHub Actions strategy.matrix.
"""

import argparse
import json
import sys

try:
    import toml
except ImportError:
    print("Error: toml package required. Install with: pip install toml", file=sys.stderr)
    sys.exit(1)


def generate_matrix(matrix_path: str, runtime_filter: str = "", version_filter: str = "") -> dict:
    """Generate build matrix from TOML configuration."""
    with open(matrix_path) as f:
        config = toml.load(f)

    includes = []

    for runtime_name, runtime_config in config.get("runtimes", {}).items():
        # Apply runtime filter if specified
        if runtime_filter and runtime_name != runtime_filter:
            continue

        versions = runtime_config.get("versions", [])
        architectures = runtime_config.get("architectures", ["x86_64"])

        for version in versions:
            # Apply version filter if specified
            if version_filter and version != version_filter:
                continue

            for arch in architectures:
                includes.append({
                    "runtime": runtime_name,
                    "version": version,
                    "arch": arch,
                })

    return {"include": includes}


def main():
    parser = argparse.ArgumentParser(description="Generate kernel build matrix")
    parser.add_argument("--matrix", required=True, help="Path to kernel-matrix.toml")
    parser.add_argument("--runtime", default="", help="Filter by runtime name")
    parser.add_argument("--version", default="", help="Filter by version")
    args = parser.parse_args()

    matrix = generate_matrix(args.matrix, args.runtime, args.version)

    # Output in GitHub Actions format
    print(f"matrix={json.dumps(matrix)}")


if __name__ == "__main__":
    main()
