#!/usr/bin/env bun
/**
 * Generate GitHub Actions build matrix from kernel-matrix.toml.
 *
 * Usage:
 *   bun scripts/generate-build-matrix.ts --matrix kernels/kernel-matrix.toml
 *
 * Outputs JSON matrix for GitHub Actions strategy.matrix.
 */

import { parseArgs } from "util";
import { readFileSync } from "fs";
import { parse as parseToml } from "smol-toml";

interface RuntimeConfig {
  versions: string[];
  architectures?: string[];
  min_memory_mib?: number;
  recommended_memory_mib?: number;
}

interface KernelMatrix {
  unikraft_version?: string;
  defaults?: {
    min_memory_mib?: number;
    recommended_memory_mib?: number;
    boot_args?: string;
  };
  runtimes?: Record<string, RuntimeConfig>;
}

interface MatrixInclude {
  runtime: string;
  version: string;
  arch: string;
}

interface BuildMatrix {
  include: MatrixInclude[];
}

function generateMatrix(
  matrixPath: string,
  runtimeFilter: string = "",
  versionFilter: string = ""
): BuildMatrix {
  const content = readFileSync(matrixPath, "utf-8");
  const config = parseToml(content) as KernelMatrix;

  const includes: MatrixInclude[] = [];

  for (const [runtimeName, runtimeConfig] of Object.entries(
    config.runtimes || {}
  )) {
    // Apply runtime filter if specified
    if (runtimeFilter && runtimeName !== runtimeFilter) {
      continue;
    }

    const versions = runtimeConfig.versions || [];
    const architectures = runtimeConfig.architectures || ["x86_64"];

    for (const version of versions) {
      // Apply version filter if specified
      if (versionFilter && version !== versionFilter) {
        continue;
      }

      for (const arch of architectures) {
        includes.push({
          runtime: runtimeName,
          version,
          arch,
        });
      }
    }
  }

  return { include: includes };
}

function main() {
  const { values } = parseArgs({
    args: process.argv.slice(2),
    options: {
      matrix: {
        type: "string",
      },
      runtime: {
        type: "string",
        default: "",
      },
      version: {
        type: "string",
        default: "",
      },
    },
    strict: true,
    allowPositionals: false,
  });

  if (!values.matrix) {
    console.error("Error: --matrix argument is required");
    process.exit(1);
  }

  const matrix = generateMatrix(
    values.matrix,
    values.runtime || "",
    values.version || ""
  );

  // Output in GitHub Actions format
  console.log(`matrix=${JSON.stringify(matrix)}`);
}

main();
