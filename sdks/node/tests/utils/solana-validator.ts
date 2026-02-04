/**
 * Solana test validator manager for Level 2 E2E tests.
 *
 * Starts solana-test-validator, deploys the Graphene Anchor program,
 * and provides RPC URL for worker configuration.
 */

import { spawn, exec, type ChildProcess } from 'child_process';
import { mkdtemp, rm, access } from 'fs/promises';
import { tmpdir } from 'os';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { promisify } from 'util';
import { GRAPHENE_PROGRAM_ID } from './solana-types.js';

const execAsync = promisify(exec);

export interface ValidatorConfig {
  /** Port for JSON-RPC (default: 8899) */
  rpcPort?: number;
  /** Port for WebSocket (default: 8900) */
  wsPort?: number;
  /** Ledger directory (default: temp directory) */
  ledgerDir?: string;
  /** Startup timeout in ms (default: 60000) */
  startupTimeoutMs?: number;
}

export interface ValidatorInstance {
  /** JSON-RPC URL (e.g., http://localhost:8899) */
  rpcUrl: string;
  /** WebSocket URL (e.g., ws://localhost:8900) */
  wsUrl: string;
  /** The underlying process */
  process: ChildProcess;
  /** Ledger directory */
  ledgerDir: string;
}

/**
 * Manages a local Solana test validator for E2E testing.
 */
export class SolanaValidator {
  private config: Required<ValidatorConfig>;
  private instance: ValidatorInstance | null = null;
  private ownedLedgerDir: boolean = false;

  constructor(config: ValidatorConfig = {}) {
    this.config = {
      rpcPort: config.rpcPort ?? 8899,
      wsPort: config.wsPort ?? 8900,
      ledgerDir: config.ledgerDir ?? '',
      startupTimeoutMs: config.startupTimeoutMs ?? 60000,
    };
  }

  /**
   * Start the Solana test validator.
   *
   * Waits for the validator to be ready before resolving.
   */
  async start(): Promise<ValidatorInstance> {
    if (this.instance) {
      throw new Error('Validator already running');
    }

    // Create temp ledger directory if not provided
    let ledgerDir = this.config.ledgerDir;
    if (!ledgerDir) {
      ledgerDir = await mkdtemp(join(tmpdir(), 'solana-test-'));
      this.ownedLedgerDir = true;
    }

    const args = [
      '--reset',
      '--ledger', ledgerDir,
      '--rpc-port', this.config.rpcPort.toString(),
      // '--quiet', // Uncomment for less verbose output
    ];

    const proc = spawn('solana-test-validator', args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    return new Promise((resolve, reject) => {
      let stderr = '';
      let stdout = '';

      const timeout = setTimeout(() => {
        proc.kill();
        reject(new Error(
          `Validator startup timeout after ${this.config.startupTimeoutMs}ms.\n` +
          `stdout: ${stdout}\nstderr: ${stderr}`
        ));
      }, this.config.startupTimeoutMs);

      proc.stdout?.on('data', (data: Buffer) => {
        const text = data.toString();
        stdout += text;
      });

      proc.stderr?.on('data', (data: Buffer) => {
        const text = data.toString();
        stderr += text;
      });

      proc.on('error', (err) => {
        clearTimeout(timeout);
        reject(new Error(`Failed to spawn solana-test-validator: ${err.message}`));
      });

      // Poll for validator readiness
      const pollReady = async () => {
        try {
          const response = await fetch(`http://localhost:${this.config.rpcPort}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              jsonrpc: '2.0',
              id: 1,
              method: 'getHealth',
            }),
          });

          if (response.ok) {
            clearTimeout(timeout);
            this.instance = {
              rpcUrl: `http://localhost:${this.config.rpcPort}`,
              wsUrl: `ws://localhost:${this.config.wsPort}`,
              process: proc,
              ledgerDir,
            };
            resolve(this.instance);
            return;
          }
        } catch {
          // Not ready yet
        }

        if (!proc.killed) {
          setTimeout(pollReady, 500);
        }
      };

      // Start polling after a short delay to give validator time to bind port
      setTimeout(pollReady, 1000);

      proc.on('exit', (code) => {
        if (!this.instance) {
          clearTimeout(timeout);
          reject(new Error(
            `Validator exited during startup with code ${code}.\n` +
            `stdout: ${stdout}\nstderr: ${stderr}`
          ));
        }
      });
    });
  }

  /**
   * Deploy the Graphene Anchor program to the local validator.
   *
   * @returns Program ID
   */
  async deployProgram(): Promise<string> {
    if (!this.instance) {
      throw new Error('Validator not running');
    }

    // Resolve path to the repo root (where Anchor.toml lives)
    const currentDir = dirname(fileURLToPath(import.meta.url));
    const repoRoot = join(currentDir, '../../../..');
    const soPath = join(repoRoot, 'programs/graphene/target/deploy/graphene.so');

    // Verify program is built
    try {
      await access(soPath);
    } catch {
      throw new Error(
        `Graphene program not built at ${soPath}. ` +
        'Run: anchor build'
      );
    }

    // Deploy using anchor deploy with custom cluster URL
    try {
      await execAsync(
        `bun anchor deploy --provider.cluster ${this.instance.rpcUrl}`,
        { cwd: repoRoot, timeout: 60000 }
      );
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      throw new Error(`Failed to deploy Graphene program: ${message}`);
    }

    const programId = GRAPHENE_PROGRAM_ID.toBase58();
    console.log(`Program deployed: ${programId}`);

    return programId;
  }

  /**
   * Stop the validator gracefully.
   */
  async stop(timeoutMs: number = 5000): Promise<void> {
    if (!this.instance) {
      return;
    }

    const proc = this.instance.process;
    const ledgerDir = this.instance.ledgerDir;

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

        // Clean up owned ledger directory
        if (this.ownedLedgerDir && ledgerDir) {
          try {
            await rm(ledgerDir, { recursive: true, force: true });
          } catch {
            // Ignore cleanup errors
          }
        }

        this.instance = null;
        this.ownedLedgerDir = false;
        resolve();
      });

      proc.kill('SIGTERM');
    });
  }

  /**
   * Get the current validator instance.
   */
  getInstance(): ValidatorInstance | null {
    return this.instance;
  }

  /**
   * Check if the validator is running.
   */
  isRunning(): boolean {
    return this.instance !== null && !this.instance.process.killed;
  }
}
