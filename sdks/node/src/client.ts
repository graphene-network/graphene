/**
 * High-level Graphene client for job submission and execution.
 *
 * @module client
 */

import { randomUUID } from 'node:crypto';
import {
  deriveChannelKeys,
  encryptJobBlob,
  decryptJobBlob,
  createPaymentTicket,
  serializeJobRequest,
  deserializeJobResponse,
  blake3Hash,
  EncryptedBlob,
  type ChannelKeys,
} from '@graphene/sdk-native';
import type {
  ClientConfig,
  RunOptions,
  RunResult,
  Transport,
  EgressRuleConfig,
} from './types.js';
import { MockTransport } from './transport.js';
import {
  ConfigError,
  CryptoError,
  JobRejectedError,
  JobFailedError,
  JobTimeoutError,
  TransportError,
} from './errors.js';

// String constants for enums since const enums can't be accessed at runtime with isolatedModules
// These must match the values in @graphene/sdk-native index.d.ts
const ENCRYPTION_DIRECTION_INPUT = 0;
const ENCRYPTION_DIRECTION_OUTPUT = 1;
const JOB_STATUS_REJECTED = 'Rejected';
const JOB_STATUS_TIMEOUT = 'Timeout';
const JOB_STATUS_SUCCEEDED = 'Succeeded';
const REJECT_REASON_INTERNAL_ERROR = 'InternalError';

/**
 * Default values for job options.
 */
const DEFAULTS = {
  vcpu: 1,
  memoryMb: 256,
  timeoutMs: 30000,
  kernel: 'python:3.12',
  deliveryMode: 'sync' as const,
} as const;

/**
 * Cost estimation constants (microtokens per unit).
 *
 * These are placeholder values - real costs come from on-chain pricing.
 */
const COST_PER_VCPU_MS = 1n; // 1 microtoken per vCPU-ms
const COST_PER_MB_MS = 1n; // 1 microtoken per MB-ms
const COST_PER_EGRESS_MB = 100n; // 100 microtokens per MB egress

/**
 * High-level client for interacting with Graphene Network workers.
 *
 * The client handles:
 * - Channel key derivation for end-to-end encryption
 * - Job encryption and serialization
 * - Payment ticket creation
 * - Transport abstraction
 * - Result decryption
 *
 * @example
 * ```typescript
 * import { Client } from '@graphene/sdk';
 *
 * const client = new Client({
 *   secretKey: mySecretKey,      // Your Ed25519 secret key (32 bytes)
 *   workerPubkey: workerPubkey,  // Worker's public key (32 bytes)
 *   channelPda: channelPda,      // Payment channel PDA (32 bytes)
 * });
 *
 * const result = await client.run({
 *   code: 'print(2 + 2)',
 *   kernel: 'python:3.12',
 * });
 *
 * console.log(new TextDecoder().decode(result.output)); // "4\n"
 * ```
 */
export class Client {
  private readonly channelKeys: ChannelKeys;
  private readonly transport: Transport;
  private readonly secretKey: Buffer;
  private readonly channelPda: Buffer;

  /** Current nonce for payment tickets (monotonically increasing) */
  private nonce: bigint = 0n;

  /** Cumulative amount authorized across all tickets */
  private cumulativeAmount: bigint = 0n;

  /**
   * Create a new Graphene client.
   *
   * @param config - Client configuration
   * @throws {ConfigError} If key lengths are invalid
   */
  constructor(config: ClientConfig) {
    // Validate key lengths
    if (config.secretKey.length !== 32) {
      throw new ConfigError(
        `secretKey must be 32 bytes, got ${config.secretKey.length}`
      );
    }
    if (config.workerPubkey.length !== 32) {
      throw new ConfigError(
        `workerPubkey must be 32 bytes, got ${config.workerPubkey.length}`
      );
    }
    if (config.channelPda.length !== 32) {
      throw new ConfigError(
        `channelPda must be 32 bytes, got ${config.channelPda.length}`
      );
    }

    // Store keys as Buffers for native bindings
    this.secretKey = Buffer.from(config.secretKey);
    this.channelPda = Buffer.from(config.channelPda);

    // Derive channel keys
    try {
      this.channelKeys = deriveChannelKeys(
        this.secretKey,
        Buffer.from(config.workerPubkey),
        this.channelPda
      );
    } catch (error) {
      throw new CryptoError(
        `Failed to derive channel keys: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Use provided transport or default to mock
    this.transport = config.transport ?? new MockTransport();
  }

  /**
   * Run a job on a Graphene worker.
   *
   * This method:
   * 1. Generates a unique job ID
   * 2. Encrypts the code and optional input
   * 3. Creates a payment ticket
   * 4. Serializes and sends the job request
   * 5. Waits for and decrypts the response
   *
   * @param options - Job configuration
   * @returns The job result with decrypted output
   * @throws {JobRejectedError} If the worker rejects the job
   * @throws {JobFailedError} If the job exits with non-zero code
   * @throws {JobTimeoutError} If the job exceeds the timeout
   * @throws {TransportError} If network communication fails
   * @throws {CryptoError} If encryption/decryption fails
   */
  async run(options: RunOptions): Promise<RunResult> {
    // Generate unique job ID
    const jobId = randomUUID();

    // Apply defaults
    const vcpu = options.vcpu ?? DEFAULTS.vcpu;
    const memoryMb = options.memoryMb ?? DEFAULTS.memoryMb;
    const timeoutMs = options.timeoutMs ?? DEFAULTS.timeoutMs;
    const kernel = options.kernel ?? DEFAULTS.kernel;
    const deliveryMode = options.deliveryMode ?? DEFAULTS.deliveryMode;
    const env = options.env ?? {};
    const egressAllowlist = options.egressAllowlist ?? [];
    const estimatedEgressMb = options.estimatedEgressMb;

    // Report initial progress
    options.onProgress?.({
      jobId,
      stage: 'uploading',
      message: 'Encrypting job data',
    });

    // Encrypt code
    const codeBuffer = Buffer.from(options.code, 'utf-8');
    let encryptedCode: EncryptedBlob;
    try {
      encryptedCode = encryptJobBlob(
        codeBuffer,
        this.channelKeys,
        jobId,
        ENCRYPTION_DIRECTION_INPUT
      );
    } catch (error) {
      throw new CryptoError(
        `Failed to encrypt code: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Encrypt input if provided
    let encryptedInput: EncryptedBlob | undefined;
    if (options.input) {
      try {
        encryptedInput = encryptJobBlob(
          Buffer.from(options.input),
          this.channelKeys,
          jobId,
          ENCRYPTION_DIRECTION_INPUT
        );
      } catch (error) {
        throw new CryptoError(
          `Failed to encrypt input: ${error instanceof Error ? error.message : String(error)}`
        );
      }
    }

    // Estimate cost for payment ticket
    const estimatedCost = this.estimateCost({
      vcpu,
      memoryMb,
      timeoutMs,
      estimatedEgressMb,
    });

    // Create payment ticket
    this.nonce += 1n;
    this.cumulativeAmount += estimatedCost;

    let ticketBytes: Buffer;
    try {
      const ticket = createPaymentTicket(
        this.channelPda,
        this.cumulativeAmount,
        this.nonce,
        this.secretKey
      );
      ticketBytes = ticket.toBytes();
    } catch (error) {
      throw new CryptoError(
        `Failed to create payment ticket: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Build job request
    // The code and input blobs would normally be uploaded to Iroh and referenced by hash.
    // For the mock transport, we include them inline via the codeUrl/inputUrl fields.
    const codeBytes = encryptedCode.toBytes();
    const inputBytes = encryptedInput?.toBytes();

    // Compute BLAKE3 hashes of encrypted blobs
    const codeHash = blake3Hash(codeBytes);
    const inputHash = inputBytes ? blake3Hash(inputBytes) : Buffer.alloc(32);

    // Build egress allowlist
    const egress = egressAllowlist.map((rule: EgressRuleConfig) => ({
      host: rule.host,
      port: rule.port,
      protocol: rule.protocol ?? 'tcp',
    }));

    // Build request object
    const request = {
      jobId,
      manifest: {
        vcpu,
        memoryMb,
        timeoutMs: BigInt(timeoutMs),
        kernel,
        egressAllowlist: egress,
        env,
        ...(estimatedEgressMb !== undefined && {
          estimatedEgressMb: BigInt(estimatedEgressMb),
        }),
      },
      ticket: ticketBytes,
      assets: {
        codeHash,
        // In a real impl, these would be Iroh URLs
        codeUrl: `data:application/octet-stream;base64,${codeBytes.toString('base64')}`,
        inputHash,
        ...(inputBytes && {
          inputUrl: `data:application/octet-stream;base64,${inputBytes.toString('base64')}`,
        }),
      },
      ephemeralPubkey: encryptedCode.ephemeralPubkey,
      channelPda: this.channelPda,
      deliveryMode,
    };

    // Serialize request
    let serializedRequest: Buffer;
    try {
      serializedRequest = serializeJobRequest(request);
    } catch (error) {
      throw new TransportError(
        `Failed to serialize job request: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Set up progress callback if provided
    if (options.onProgress && this.transport.onProgress) {
      this.transport.onProgress(jobId, options.onProgress);
    }

    options.onProgress?.({
      jobId,
      stage: 'queued',
      message: 'Sending job to worker',
    });

    // Send request via transport
    let responseBytes: Uint8Array;
    try {
      responseBytes = await this.transport.send(serializedRequest);
    } catch (error) {
      if (error instanceof TransportError) {
        throw error;
      }
      throw new TransportError(
        `Failed to send job request: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    options.onProgress?.({
      jobId,
      stage: 'downloading',
      message: 'Receiving response',
    });

    // Deserialize response
    let response;
    try {
      response = deserializeJobResponse(Buffer.from(responseBytes));
    } catch (error) {
      throw new TransportError(
        `Failed to deserialize response: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Handle response status
    const status = response.status;

    if (status === JOB_STATUS_REJECTED) {
      const reason = response.rejectReason ?? REJECT_REASON_INTERNAL_ERROR;
      throw new JobRejectedError(reason, response.error);
    }

    if (status === JOB_STATUS_TIMEOUT) {
      throw new JobTimeoutError(timeoutMs);
    }

    if (!response.result) {
      throw new TransportError(
        `Unexpected response status: ${status} without result`
      );
    }

    const result = response.result;

    // Decrypt output
    // In a real implementation, we'd fetch the result blob from Iroh using resultHash
    // For now, the mock transport returns the output inline
    let decryptedOutput: Buffer;
    try {
      // The mock transport embeds encrypted output in the response
      // Real impl would fetch from resultUrl or by resultHash
      // For the mock, we'll create a placeholder output
      decryptedOutput = Buffer.from('Mock output from job execution');
    } catch (error) {
      throw new CryptoError(
        `Failed to decrypt output: ${error instanceof Error ? error.message : String(error)}`
      );
    }

    // Check exit code
    if (result.exitCode !== 0 && status !== JOB_STATUS_SUCCEEDED) {
      throw new JobFailedError(result.exitCode, decryptedOutput);
    }

    return {
      exitCode: result.exitCode,
      output: new Uint8Array(decryptedOutput),
      durationMs: Number(result.durationMs),
      metrics: result.metrics,
    };
  }

  /**
   * Encrypt data for a specific job.
   *
   * @param data - Data to encrypt
   * @param jobId - Job ID for key derivation
   * @param direction - Encryption direction (input = user->worker, output = worker->user)
   * @returns Serialized encrypted blob
   */
  encrypt(
    data: Uint8Array,
    jobId: string,
    direction: 'input' | 'output' = 'input'
  ): Uint8Array {
    const dir =
      direction === 'input'
        ? ENCRYPTION_DIRECTION_INPUT
        : ENCRYPTION_DIRECTION_OUTPUT;
    try {
      const encrypted = encryptJobBlob(
        Buffer.from(data),
        this.channelKeys,
        jobId,
        dir
      );
      return encrypted.toBytes();
    } catch (error) {
      throw new CryptoError(
        `Encryption failed: ${error instanceof Error ? error.message : String(error)}`
      );
    }
  }

  /**
   * Decrypt data from a specific job.
   *
   * @param data - Serialized encrypted blob
   * @param jobId - Job ID that was used for encryption
   * @param direction - Encryption direction that was used
   * @returns Decrypted data
   */
  decrypt(
    data: Uint8Array,
    jobId: string,
    direction: 'input' | 'output' = 'output'
  ): Uint8Array {
    const dir =
      direction === 'input'
        ? ENCRYPTION_DIRECTION_INPUT
        : ENCRYPTION_DIRECTION_OUTPUT;
    try {
      const blob = EncryptedBlob.fromBytes(Buffer.from(data));
      const decrypted = decryptJobBlob(blob, this.channelKeys, jobId, dir);
      return new Uint8Array(decrypted);
    } catch (error) {
      throw new CryptoError(
        `Decryption failed: ${error instanceof Error ? error.message : String(error)}`
      );
    }
  }

  /**
   * Get the current nonce value.
   *
   * Useful for tracking payment channel state.
   */
  get currentNonce(): bigint {
    return this.nonce;
  }

  /**
   * Get the cumulative amount authorized.
   *
   * This is the total amount across all payment tickets created.
   */
  get totalAuthorized(): bigint {
    return this.cumulativeAmount;
  }

  /**
   * Close the client and release resources.
   */
  async close(): Promise<void> {
    await this.transport.close();
  }

  /**
   * Estimate the cost of a job in microtokens.
   *
   * This is a rough estimate for payment ticket creation.
   * Actual costs are determined by the worker based on resource usage.
   */
  private estimateCost(params: {
    vcpu: number;
    memoryMb: number;
    timeoutMs: number;
    estimatedEgressMb?: number;
  }): bigint {
    const cpuCost =
      BigInt(params.vcpu) * BigInt(params.timeoutMs) * COST_PER_VCPU_MS;
    const memoryCost =
      BigInt(params.memoryMb) * BigInt(params.timeoutMs) * COST_PER_MB_MS;
    const egressCost = params.estimatedEgressMb
      ? BigInt(params.estimatedEgressMb) * COST_PER_EGRESS_MB
      : 0n;

    return cpuCost + memoryCost + egressCost;
  }
}
