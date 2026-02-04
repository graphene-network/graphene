/**
 * High-level Graphene client for job submission and execution.
 *
 * This is a thin wrapper around the native Rust client.
 * All business logic (encryption, hashing, transport, etc.) is in Rust.
 *
 * @module client
 */

import {
  GrapheneClient as NativeClient,
  type ClientConfig as NativeClientConfig,
  type JobOptions as NativeJobOptions,
  type NativeJobResult,
  type EgressRule,
} from '@graphene/sdk-native';
import type { ClientConfig, RunOptions, RunResult } from './types.js';
import { ConfigError } from './errors.js';

/**
 * High-level client for interacting with Graphene Network workers.
 *
 * This is a thin TypeScript wrapper around the native Rust client.
 * All cryptography, networking, and protocol handling is done in Rust.
 *
 * @example
 * ```typescript
 * import { Client } from '@graphene/sdk';
 *
 * const client = await Client.create({
 *   secretKey: mySecretKey,  // Your Ed25519 secret key (32 bytes)
 *   channelPda: channelPda,  // Payment channel PDA (32 bytes)
 *   workerNodeId: nodeId,    // Worker's node ID (hex-encoded Ed25519 pubkey)
 * });
 *
 * const result = await client.run({
 *   code: 'print(2 + 2)',
 *   kernel: 'python:3.12',
 * });
 *
 * console.log(new TextDecoder().decode(result.output)); // "4\n"
 *
 * await client.close();
 * ```
 */
export class Client {
  private readonly native: NativeClient;

  private constructor(native: NativeClient) {
    this.native = native;
  }

  /**
   * Create a new Graphene client.
   *
   * @param config - Client configuration
   * @throws {ConfigError} If key lengths are invalid
   */
  static async create(config: ClientConfig): Promise<Client> {
    // Validate key lengths
    if (config.secretKey.length !== 32) {
      throw new ConfigError(
        `secretKey must be 32 bytes, got ${config.secretKey.length}`
      );
    }
    if (config.channelPda.length !== 32) {
      throw new ConfigError(
        `channelPda must be 32 bytes, got ${config.channelPda.length}`
      );
    }
    if (!config.workerNodeId || config.workerNodeId.length !== 64) {
      throw new ConfigError(
        'workerNodeId must be a 64-character hex string (Ed25519 public key)'
      );
    }

    const nativeConfig: NativeClientConfig = {
      storagePath: config.storagePath ?? '.graphene-sdk',
      secretKey: Buffer.from(config.secretKey),
      channelPda: Buffer.from(config.channelPda),
      workerNodeId: config.workerNodeId,
      useRelay: config.useRelay ?? true,
      bindPort: config.bindPort,
    };

    const native = await NativeClient.create(nativeConfig);
    return new Client(native);
  }

  /**
   * Run a job on a Graphene worker.
   *
   * @param options - Job configuration
   * @returns The job result with decrypted output
   */
  async run(options: RunOptions): Promise<RunResult> {
    // Convert egress allowlist to native format
    const egressAllowlist: EgressRule[] | undefined = options.networking?.egressAllowlist?.map(
      (rule) => ({
        host: rule.host,
        port: rule.port,
        protocol: rule.protocol ?? 'tcp',
      })
    );

    const nativeOptions: NativeJobOptions = {
      code: options.code,
      input: options.input ? Buffer.from(options.input) : undefined,
      resources: options.resources ? {
        vcpu: options.resources.vcpu,
        memoryMb: options.resources.memoryMb,
      } : undefined,
      networking: options.networking ? {
        estimatedIngressMb: options.networking.estimatedIngressMb !== undefined
          ? BigInt(options.networking.estimatedIngressMb)
          : undefined,
        estimatedEgressMb: options.networking.estimatedEgressMb !== undefined
          ? BigInt(options.networking.estimatedEgressMb)
          : undefined,
        egressAllowlist,
      } : undefined,
      assets: options.assets ? {
        mode: options.assets.mode,
        inlineCodeThreshold: options.assets.inlineCodeThreshold,
        inlineInputThreshold: options.assets.inlineInputThreshold,
        compress: options.assets.compress,
      } : undefined,
      timeoutMs: options.timeoutMs !== undefined ? BigInt(options.timeoutMs) : undefined,
      kernel: options.kernel,
      env: options.env,
      deliveryMode: options.deliveryMode,
    };

    const result: NativeJobResult = await this.native.submitJob(nativeOptions);

    return {
      exitCode: result.exitCode,
      output: new Uint8Array(result.output),
      durationMs: Number(result.durationMs),
      metrics: {
        peakMemoryBytes: result.metrics.peakMemoryBytes,
        cpuTimeMs: result.metrics.cpuTimeMs,
        networkRxBytes: result.metrics.networkRxBytes,
        networkTxBytes: result.metrics.networkTxBytes,
        totalCostMicros: result.metrics.totalCostMicros,
        cpuCostMicros: result.metrics.cpuCostMicros,
        memoryCostMicros: result.metrics.memoryCostMicros,
        egressCostMicros: result.metrics.egressCostMicros,
      },
    };
  }

  /**
   * Get this client's node ID (public key) as a hex string.
   */
  async nodeId(): Promise<string> {
    return this.native.nodeId();
  }

  /**
   * Get the current nonce value.
   */
  get currentNonce(): bigint {
    return this.native.currentNonce;
  }

  /**
   * Get the cumulative amount authorized.
   */
  get totalAuthorized(): bigint {
    return this.native.totalAuthorized;
  }

  /**
   * Close the client and release resources.
   */
  async close(): Promise<void> {
    await this.native.shutdown();
  }
}
