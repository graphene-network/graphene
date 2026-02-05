/**
 * E2E tests for TypeScript SDK ↔ Graphene Worker communication.
 *
 * Level 1: Mock Channel Tests
 * - Tests QUIC protocol, encryption, job execution
 * - Uses GRAPHENE_TEST_USER_PUBKEY for test channel injection
 * - No Solana dependencies
 *
 * Runner handling:
 * - Linux + Firecracker + /dev/kvm: Uses FirecrackerRunner (returns actual execution output)
 * - Otherwise: Uses MockRunner (returns "Mock execution completed\n")
 */

import { describe, it, expect, beforeAll, afterAll } from 'bun:test';
import { Client } from '../src/client.js';
import { WorkerManager, type WorkerInstance } from './utils/worker-manager.js';
import { generateTestKeypair, testChannelPda, type TestKeypair } from './utils/test-keys.js';
import { shouldUseMockRunner } from './utils/vmm-detection.js';

// Runner detection for expected outputs
const USE_MOCK_RUNNER = await shouldUseMockRunner();
const MOCK_OUTPUT = 'Mock execution completed\n';

// Test timeout (longer for worker startup)
const TEST_TIMEOUT = 60_000;

function readTarEntry(archive: Uint8Array, name: string): Uint8Array | null {
  const decoder = new TextDecoder();
  let offset = 0;

  while (offset + 512 <= archive.length) {
    const header = archive.subarray(offset, offset + 512);

    let allZero = true;
    for (let i = 0; i < header.length; i += 1) {
      if (header[i] !== 0) {
        allZero = false;
        break;
      }
    }
    if (allZero) break;

    const fileName = decoder.decode(header.subarray(0, 100)).replace(/\0.*$/, '');
    const sizeOctal = decoder.decode(header.subarray(124, 136)).replace(/\0.*$/, '').trim();
    const size = sizeOctal ? parseInt(sizeOctal, 8) : 0;

    offset += 512;
    const fileData = archive.subarray(offset, offset + size);
    const padding = (512 - (size % 512)) % 512;
    offset += size + padding;

    if (fileName === name) {
      return fileData;
    }
  }

  return null;
}

describe('E2E: Mock Channel Tests', () => {
  let workerManager: WorkerManager;
  let worker: WorkerInstance;
  let testKeypair: TestKeypair;

  beforeAll(async () => {
    // Generate test keypair
    testKeypair = await generateTestKeypair();
    console.log(`Test user pubkey: ${testKeypair.publicKeyHex}`);

    // Start worker with test channel configured
    workerManager = new WorkerManager({
      testUserPubkeyHex: testKeypair.publicKeyHex,
      startupTimeoutMs: 45_000,
    });

    console.log('Starting worker...');
    worker = await workerManager.start();
    console.log(`Worker started with node ID: ${worker.nodeId}`);
  }, TEST_TIMEOUT);

  afterAll(async () => {
    console.log('Stopping worker...');
    await workerManager.stop();
    console.log('Worker stopped');
  }, TEST_TIMEOUT);

  describe('Happy Path', () => {
    it('executes a Python job successfully', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'print("Hello from Python!")',
          kernel: 'python:3.12',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          // MockRunner returns fixed output
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          // Real execution returns actual output
          expect(output).toContain('Hello from Python!');
        }

        // Verify metrics are populated
        expect(result.durationMs).toBeGreaterThan(0);
        expect(result.metrics.totalCostMicros).toBeGreaterThanOrEqual(0n);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('executes a Node.js job successfully', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'console.log("Hello from Node!")',
          kernel: 'node:21',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Hello from Node!');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('passes environment variables to the job', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'import os; print(os.environ.get("MY_VAR", "NOT_SET"))',
          kernel: 'python:3.12',
          env: { MY_VAR: 'test_value_123' },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('test_value_123');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('handles sequential jobs with incrementing nonces', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const initialNonce = client.currentNonce;

        // Run first job
        const result1 = await client.run({
          code: 'print(1)',
          kernel: 'python:3.12',
        });
        expect(result1.exitCode).toBe(0);
        expect(client.currentNonce).toBe(initialNonce + 1n);

        // Run second job
        const result2 = await client.run({
          code: 'print(2)',
          kernel: 'python:3.12',
        });
        expect(result2.exitCode).toBe(0);
        expect(client.currentNonce).toBe(initialNonce + 2n);

        // Run third job
        const result3 = await client.run({
          code: 'print(3)',
          kernel: 'python:3.12',
        });
        expect(result3.exitCode).toBe(0);
        expect(client.currentNonce).toBe(initialNonce + 3n);

        // Verify total authorized is increasing
        expect(client.totalAuthorized).toBeGreaterThan(0n);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('handles custom resource requests', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'print("With resources")',
          kernel: 'python:3.12',
          resources: {
            vcpu: 2,
            memoryMb: 512,
          },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });

  describe('Error Handling', () => {
    it('rejects unsupported kernel', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        await expect(
          client.run({
            code: 'print("test")',
            kernel: 'ruby:3.2', // Not in supported kernels list
            timeoutMs: 10_000,
          })
        ).rejects.toThrow(/UnsupportedKernel|not supported/i);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('rejects reserved GRAPHENE_* environment variable prefix', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        await expect(
          client.run({
            code: 'print("test")',
            kernel: 'python:3.12',
            env: { GRAPHENE_INTERNAL: 'forbidden' },
            timeoutMs: 10_000,
          })
        ).rejects.toThrow(/ReservedEnvPrefix|GRAPHENE_|reserved/i);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('rejects excessive resource requests', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        await expect(
          client.run({
            code: 'print("test")',
            kernel: 'python:3.12',
            resources: {
              vcpu: 100, // Way over max_vcpu: 4
              memoryMb: 100_000, // Way over max_memory_mb: 4096
            },
            timeoutMs: 10_000,
          })
        ).rejects.toThrow(/ResourcesExceedLimits|exceed|limit/i);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('rejects wrong user key (signature mismatch)', async () => {
      // Generate a different keypair (won't match test channel)
      const wrongKeypair = await generateTestKeypair();

      const client = await Client.create({
        secretKey: wrongKeypair.secretKey, // Different from test channel user
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        await expect(
          client.run({
            code: 'print("test")',
            kernel: 'python:3.12',
            timeoutMs: 10_000,
          })
        ).rejects.toThrow(/TicketInvalid|signature|invalid/i);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });

  describe('Protocol Verification', () => {
    it('client reports correct node ID format', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const clientNodeId = await client.nodeId();

        // Node ID should be a 64-character hex string
        expect(clientNodeId).toMatch(/^[a-f0-9]{64}$/i);
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('worker node ID matches expected format', () => {
      // Worker node ID should be a 64-character hex string (Ed25519 pubkey)
      expect(worker.nodeId).toMatch(/^[a-f0-9]{64}$/i);
    });
  });

  describe('Asset Delivery Modes', () => {
    it('uses auto mode by default (inlines small payloads)', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        // Small code should be delivered inline by default (auto mode)
        const result = await client.run({
          code: 'print("Auto mode inline delivery")',
          kernel: 'python:3.12',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Auto mode inline delivery');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('accepts explicit inline mode for small payloads', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: 'print("Explicit inline mode")',
          kernel: 'python:3.12',
          assets: {
            mode: 'inline',
          },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Explicit inline mode');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    // TODO(#158): Enable after remote blob downloads are supported in GrapheneNode::download_blob.
    it.skip('accepts explicit blob mode', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        // Blob mode should upload to Iroh and then be fetched by worker
        const result = await client.run({
          code: 'print("Explicit blob mode")',
          kernel: 'python:3.12',
          assets: {
            mode: 'blob',
          },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Explicit blob mode');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('handles compression enabled', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        // Compression should reduce payload size before encryption
        // Using repetitive code to benefit from compression
        const repetitiveCode = `
# Repetitive data that compresses well
data = "AAAAAAAAAA" * 1000
print(f"Compressed delivery: {len(data)} chars")
`.trim();

        const result = await client.run({
          code: repetitiveCode,
          kernel: 'python:3.12',
          assets: {
            compress: true,
          },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Compressed delivery: 10000 chars');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('handles input data with inline delivery', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const inputData = new TextEncoder().encode('Hello from input data!');

        const result = await client.run({
          code: `
import sys
# Read input from stdin
input_data = sys.stdin.read()
print(f"Received input: {input_data}")
`.trim(),
          kernel: 'python:3.12',
          input: inputData,
          assets: {
            mode: 'inline',
          },
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('Received input: Hello from input data!');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });

  describe('Inline Results And Streams', () => {
    it('returns stderr when stdout is empty', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: `
import sys
sys.stderr.write("stderr-only\\n")
`.trim(),
          kernel: 'python:3.12',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        const output = new TextDecoder().decode(result.output);

        if (USE_MOCK_RUNNER) {
          expect(output).toBe(MOCK_OUTPUT);
        } else {
          expect(output).toContain('stderr-only');
        }
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);

    it('returns inline result tarball when stdout/stderr are empty', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      try {
        const result = await client.run({
          code: `
from pathlib import Path
Path("/output/inline.txt").write_text("inline-result-ok")
`.trim(),
          kernel: 'python:3.12',
          timeoutMs: 30_000,
        });

        expect(result.exitCode).toBe(0);

        if (USE_MOCK_RUNNER) {
          const output = new TextDecoder().decode(result.output);
          expect(output).toBe(MOCK_OUTPUT);
          return;
        }

        const entry = readTarEntry(result.output, 'inline.txt');
        expect(entry).not.toBeNull();

        const contents = new TextDecoder().decode(entry!);
        expect(contents).toBe('inline-result-ok');
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });

  describe('Benchmark', () => {
    it('runs the same job twice and reports timings', async () => {
      const client = await Client.create({
        secretKey: testKeypair.secretKey,
        channelPda: testChannelPda(),
        workerNodeId: worker.nodeId,
        relayUrl: worker.relayUrl ?? undefined,
        storagePath: `.graphene-test-${Date.now()}`,
      });

      const job = {
        code: 'print("benchmark-run")',
        kernel: 'python:3.12',
        timeoutMs: 30_000,
      } as const;

      try {
        const startFirst = performance.now();
        const first = await client.run(job);
        const firstElapsedMs = performance.now() - startFirst;

        const startSecond = performance.now();
        const second = await client.run(job);
        const secondElapsedMs = performance.now() - startSecond;

        expect(first.exitCode).toBe(0);
        expect(second.exitCode).toBe(0);

        console.log(
          `Benchmark timings (ms): first wall=${firstElapsedMs.toFixed(2)}, ` +
          `second wall=${secondElapsedMs.toFixed(2)}, ` +
          `first exec=${first.durationMs}, second exec=${second.durationMs}`
        );
      } finally {
        await client.close();
      }
    }, TEST_TIMEOUT);
  });
});
