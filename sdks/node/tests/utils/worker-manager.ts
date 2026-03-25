/**
 * Worker process manager for E2E tests.
 *
 * Spawns the OpenCapsule worker binary and manages its lifecycle.
 */

import { spawn, type ChildProcess } from 'child_process';
import { mkdtemp, rm } from 'fs/promises';
import { tmpdir } from 'os';
import { join } from 'path';

// Pipe worker stdout/stderr into test output by default.
// Set OPENCAPSULE_E2E_VERBOSE_WORKER=0 to silence.
const VERBOSE_WORKER_LOGS = process.env.OPENCAPSULE_E2E_VERBOSE_WORKER !== '0';

export interface WorkerConfig {
  /** Path to the worker binary (default: cargo run --bin server) */
  binaryPath?: string;
  /** Test user public key (hex string) for OPENCAPSULE_TEST_USER_PUBKEY */
  testUserPubkeyHex: string;
  /** Storage path (default: temp directory) */
  storagePath?: string;
  /** Startup timeout in ms (default: 30000) */
  startupTimeoutMs?: number;
  /** Additional environment variables */
  env?: Record<string, string>;
}

export interface WorkerInstance {
  /** The worker's node ID (hex-encoded Ed25519 pubkey) */
  nodeId: string;
  /** The worker's relay URL for NAT traversal (may be null if no relay) */
  relayUrl: string | null;
  /** The underlying child process */
  process: ChildProcess;
  /** Storage path used by this worker */
  storagePath: string;
}

/**
 * Manages a OpenCapsule worker process for E2E testing.
 */
export class WorkerManager {
  private config: Required<WorkerConfig>;
  private instance: WorkerInstance | null = null;
  private ownedStoragePath: boolean = false;

  constructor(config: WorkerConfig) {
    // Allow env var to override binary path (useful for CI with pre-built binary)
    const envBinary = process.env.OPENCAPSULE_WORKER_BINARY;
    this.config = {
      binaryPath: config.binaryPath ?? envBinary ?? 'cargo',
      testUserPubkeyHex: config.testUserPubkeyHex,
      storagePath: config.storagePath ?? '',
      startupTimeoutMs: config.startupTimeoutMs ?? 30000,
      env: config.env ?? {},
    };
  }

  /**
   * Start the worker process.
   *
   * Waits for the worker to print its node ID and "Listening for job requests"
   * before resolving.
   *
   * @returns Worker instance with node ID and process handle
   */
  async start(): Promise<WorkerInstance> {
    if (this.instance) {
      throw new Error('Worker already running');
    }

    // Create temp storage if not provided
    let storagePath = this.config.storagePath;
    if (!storagePath) {
      storagePath = await mkdtemp(join(tmpdir(), 'opencapsule-test-'));
      this.ownedStoragePath = true;
    }

    const env: Record<string, string> = {
      ...process.env as Record<string, string>,
      OPENCAPSULE_STORAGE_PATH: storagePath,
      OPENCAPSULE_TEST_USER_PUBKEY: this.config.testUserPubkeyHex,
      RUST_LOG: 'opencapsule_node=debug,opencapsule_worker=debug',
      OPENCAPSULE_KERNELS: process.env.OPENCAPSULE_KERNELS ?? 'python:3.12,node:21',
      ...this.config.env,
    };

    // Spawn the worker process
    const args = this.config.binaryPath === 'cargo'
      ? ['run', '--bin', 'opencapsule-worker', '--quiet']
      : [];

    const proc = spawn(this.config.binaryPath, args, {
      cwd: join(import.meta.dir, '../../../../crates/node'),
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    // Parse node ID, relay URL, and wait for ready signal
    return new Promise((resolve, reject) => {
      let nodeId: string | null = null;
      let relayUrl: string | null = null;
      let ready = false;
      let stderr = '';
      let stdout = '';

      const timeout = setTimeout(() => {
        proc.kill();
        reject(new Error(
          `Worker startup timeout after ${this.config.startupTimeoutMs}ms.\n` +
          `stdout: ${stdout}\nstderr: ${stderr}`
        ));
      }, this.config.startupTimeoutMs);

      proc.stdout?.on('data', (data: Buffer) => {
        const text = data.toString();
        stdout += text;

        if (VERBOSE_WORKER_LOGS) {
          process.stdout.write(`[worker stdout] ${text}`);
        }

        // Parse node ID from "workerNodeId: ..." line
        const nodeIdMatch = text.match(/workerNodeId:\s*"([a-f0-9]{64})"/i);
        if (nodeIdMatch) {
          nodeId = nodeIdMatch[1];
        }

        // Parse relay URL from "relayUrl: ..." line
        const relayMatch = text.match(/relayUrl:\s*"([^"]+)"/);
        if (relayMatch) {
          relayUrl = relayMatch[1];
        }

        // Check for ready signal
        if (text.includes('Listening for job requests')) {
          ready = true;
        }

        // Resolve when we have both nodeId and ready (relayUrl is optional)
        if (nodeId && ready) {
          clearTimeout(timeout);
          this.instance = {
            nodeId,
            relayUrl,
            process: proc,
            storagePath,
          };
          resolve(this.instance);
        }
      });

      proc.stderr?.on('data', (data: Buffer) => {
        const text = data.toString();
        stderr += text;

        if (VERBOSE_WORKER_LOGS) {
          process.stdout.write(`[worker stderr] ${text}`);
        }

        // Also check stderr for tracing output
        const nodeIdMatch = text.match(/workerNodeId:\s*"([a-f0-9]{64})"/i);
        if (nodeIdMatch) {
          nodeId = nodeIdMatch[1];
        }

        // Parse relay URL from stderr too (tracing outputs here)
        const relayMatch = text.match(/relayUrl:\s*"([^"]+)"/);
        if (relayMatch) {
          relayUrl = relayMatch[1];
        }

        if (text.includes('Listening for job requests')) {
          ready = true;
        }

        if (nodeId && ready) {
          clearTimeout(timeout);
          this.instance = {
            nodeId,
            relayUrl,
            process: proc,
            storagePath,
          };
          resolve(this.instance);
        }
      });

      proc.on('error', (err) => {
        clearTimeout(timeout);
        reject(new Error(`Failed to spawn worker: ${err.message}`));
      });

      proc.on('exit', (code) => {
        if (!this.instance) {
          clearTimeout(timeout);
          reject(new Error(
            `Worker exited during startup with code ${code}.\n` +
            `stdout: ${stdout}\nstderr: ${stderr}`
          ));
        }
      });
    });
  }

  /**
   * Stop the worker process gracefully.
   *
   * Sends SIGTERM first, then SIGKILL after timeout.
   *
   * @param timeoutMs - Time to wait for graceful shutdown (default: 5000)
   */
  async stop(timeoutMs: number = 5000): Promise<void> {
    if (!this.instance) {
      return;
    }

    const proc = this.instance.process;
    const storagePath = this.instance.storagePath;

    return new Promise((resolve) => {
      let killed = false;

      const forceKill = setTimeout(() => {
        if (!killed) {
          proc.kill('SIGKILL');
        }
      }, timeoutMs);

      proc.on('exit', async () => {
        killed = true;
        clearTimeout(forceKill);

        // Clean up owned storage path
        if (this.ownedStoragePath && storagePath) {
          try {
            await rm(storagePath, { recursive: true, force: true });
          } catch {
            // Ignore cleanup errors
          }
        }

        this.instance = null;
        this.ownedStoragePath = false;
        resolve();
      });

      // Send SIGTERM for graceful shutdown
      proc.kill('SIGTERM');
    });
  }

  /**
   * Get the current worker instance.
   */
  getInstance(): WorkerInstance | null {
    return this.instance;
  }

  /**
   * Check if the worker is running.
   */
  isRunning(): boolean {
    return this.instance !== null && !this.instance.process.killed;
  }
}
