#!/usr/bin/env bun
/**
 * Generate kernel metadata JSON from built kernel binary.
 *
 * Usage:
 *   bun scripts/generate-kernel-metadata.ts \
 *     --runtime python \
 *     --version 3.11 \
 *     --arch x86_64 \
 *     --kernel-path path/to/kernel.fc \
 *     --output python-3.11-x86_64.json
 */

import { parseArgs } from "util";
import { readFileSync, writeFileSync, statSync, readdirSync } from "fs";
import { parse as parseToml } from "smol-toml";
import { join, dirname, basename } from "path";
import { createHash } from "crypto";
import { Glob } from "bun";

interface KernelMatrix {
  unikraft_version?: string;
  defaults?: {
    min_memory_mib?: number;
    recommended_memory_mib?: number;
    boot_args?: string;
  };
  runtimes?: Record<
    string,
    {
      versions?: string[];
      architectures?: string[];
      min_memory_mib?: number;
      recommended_memory_mib?: number;
    }
  >;
}

interface KernelMetadata {
  spec: {
    runtime: string;
    version: string;
    arch: string;
    variant: null;
  };
  binary_hash: string;
  binary_size_bytes: number;
  min_memory_mib: number;
  recommended_memory_mib: number;
  default_boot_args: string;
  unikraft_version: string;
  built_at: string;
}

function calculateHash(filePath: string): string {
  // Use SHA256 for hashing
  const fileContent = readFileSync(filePath);
  return createHash("sha256").update(fileContent).digest("hex");
}

function getMemoryConfig(
  matrixPath: string,
  runtime: string
): [number, number] {
  try {
    const content = readFileSync(matrixPath, "utf-8");
    const config = parseToml(content) as KernelMatrix;

    const defaults = config.defaults || {};
    const defaultMin = defaults.min_memory_mib || 128;
    const defaultRec = defaults.recommended_memory_mib || 256;

    const runtimeConfig = config.runtimes?.[runtime] || {};
    const minMem = runtimeConfig.min_memory_mib || defaultMin;
    const recMem = runtimeConfig.recommended_memory_mib || defaultRec;

    return [minMem, recMem];
  } catch {
    return [128, 256];
  }
}

function findKernelFile(kernelPath: string): string {
  try {
    // Check if the path exists directly
    statSync(kernelPath);
    return kernelPath;
  } catch {
    // Try to find it using glob pattern
    const dir = dirname(kernelPath);
    const pattern = basename(kernelPath);

    try {
      const files = readdirSync(dir);
      const glob = new Glob(pattern);

      for (const file of files) {
        if (glob.match(file)) {
          return join(dir, file);
        }
      }
    } catch {
      // Directory doesn't exist
    }

    throw new Error(`Kernel file not found: ${kernelPath}`);
  }
}

function generateMetadata(
  runtime: string,
  version: string,
  arch: string,
  kernelPath: string,
  matrixPath: string = "kernels/kernel-matrix.toml"
): KernelMetadata {
  // Find the actual kernel file
  const kernelFile = findKernelFile(kernelPath);

  // Calculate hash and size
  const binaryHash = calculateHash(kernelFile);
  const binarySize = statSync(kernelFile).size;

  // Get memory config from matrix
  const [minMem, recMem] = getMemoryConfig(matrixPath, runtime);

  // Get Unikraft version from matrix
  let unikraftVersion = "0.17.0";
  try {
    const content = readFileSync(matrixPath, "utf-8");
    const config = parseToml(content) as KernelMatrix;
    unikraftVersion = config.unikraft_version || unikraftVersion;
  } catch {
    // Use default
  }

  return {
    spec: {
      runtime,
      version,
      arch,
      variant: null,
    },
    binary_hash: binaryHash,
    binary_size_bytes: binarySize,
    min_memory_mib: minMem,
    recommended_memory_mib: recMem,
    default_boot_args:
      "console=ttyS0 noapic reboot=k panic=1 pci=off nomodules",
    unikraft_version: unikraftVersion,
    built_at: new Date().toISOString(),
  };
}

async function main() {
  const { values } = parseArgs({
    args: process.argv.slice(2),
    options: {
      runtime: {
        type: "string",
      },
      version: {
        type: "string",
      },
      arch: {
        type: "string",
      },
      "kernel-path": {
        type: "string",
      },
      matrix: {
        type: "string",
        default: "kernels/kernel-matrix.toml",
      },
      output: {
        type: "string",
      },
    },
    strict: true,
    allowPositionals: false,
  });

  if (!values.runtime || !values.version || !values.arch || !values["kernel-path"] || !values.output) {
    console.error("Error: --runtime, --version, --arch, --kernel-path, and --output are required");
    process.exit(1);
  }

  const metadata = generateMetadata(
    values.runtime,
    values.version,
    values.arch,
    values["kernel-path"],
    values.matrix || "kernels/kernel-matrix.toml"
  );

  writeFileSync(values.output, JSON.stringify(metadata, null, 2));

  console.log(`Generated metadata: ${values.output}`);
}

main();
