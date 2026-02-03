/**
 * Type definitions for the Graphene SDK.
 *
 * Re-exports all types from the native bindings and adds additional
 * TypeScript-specific types for the high-level client.
 *
 * @module types
 */

// Re-export types from native bindings
// Note: EncryptionDirection, JobStatus, RejectReason are const enums
// and must be re-exported with their values for runtime use
export type {
  ChannelKeys,
  EncryptedBlob,
  PaymentTicket,
  ChannelState,
  JobRequest,
  JobResponse,
  JobManifest,
  JobAssets,
  JobResult,
  JobMetrics,
  EgressRule,
  WireMessage,
} from '@graphene/sdk-native';

// Re-export const enums (these need special handling)
export {
  EncryptionDirection,
  JobStatus,
  RejectReason,
} from '@graphene/sdk-native';

/**
 * Progress information for a running job.
 */
export interface JobProgress {
  /** The job ID this progress refers to */
  jobId: string;
  /** Current stage of job execution */
  stage: 'queued' | 'running' | 'uploading' | 'downloading';
  /** Progress percentage (0-100), if available */
  progress?: number;
  /** Human-readable status message */
  message?: string;
}

/**
 * Configuration options for the Graphene Client.
 */
export interface ClientConfig {
  /** Ed25519 secret key (32 bytes) */
  secretKey: Uint8Array;
  /** Worker's Ed25519 public key (32 bytes) */
  workerPubkey: Uint8Array;
  /** Solana payment channel PDA (32 bytes) */
  channelPda: Uint8Array;
  /** Optional transport implementation (default: MockTransport for testing) */
  transport?: Transport;
}

/**
 * Options for running a job.
 */
export interface RunOptions {
  /** Code to execute (will be encrypted) */
  code: string;
  /** Optional input data (will be encrypted) */
  input?: Uint8Array;
  /** Number of vCPUs (default: 1) */
  vcpu?: number;
  /** Memory in MB (default: 256) */
  memoryMb?: number;
  /** Execution timeout in milliseconds (default: 30000) */
  timeoutMs?: number;
  /** Kernel image to use (default: "python:3.12") */
  kernel?: string;
  /** Environment variables to pass to the job */
  env?: Record<string, string>;
  /** Allowed egress endpoints */
  egressAllowlist?: EgressRuleConfig[];
  /** Estimated network egress in MB (for cost estimation) */
  estimatedEgressMb?: number;
  /** Delivery mode: "sync" waits for result, "async" returns immediately */
  deliveryMode?: 'sync' | 'async';
  /** Callback for job progress updates */
  onProgress?: (progress: JobProgress) => void;
}

/**
 * Simplified egress rule configuration.
 */
export interface EgressRuleConfig {
  /** Hostname or IP address */
  host: string;
  /** Port number */
  port: number;
  /** Protocol (default: "tcp") */
  protocol?: 'tcp' | 'udp';
}

/**
 * Result of a successful job execution.
 */
export interface RunResult {
  /** Exit code from the job (0 = success) */
  exitCode: number;
  /** Decrypted output data */
  output: Uint8Array;
  /** Execution duration in milliseconds */
  durationMs: number;
  /** Resource usage metrics */
  metrics: import('@graphene/sdk-native').JobMetrics;
}

/**
 * Transport interface for sending jobs to workers.
 *
 * Implementations handle the actual network communication.
 */
export interface Transport {
  /**
   * Send a job request and receive the response.
   *
   * @param request - Wire-formatted job request bytes
   * @returns Wire-formatted job response bytes
   */
  send(request: Uint8Array): Promise<Uint8Array>;

  /**
   * Subscribe to progress updates for a job.
   *
   * @param jobId - The job ID to subscribe to
   * @param callback - Called with progress updates
   */
  onProgress?(jobId: string, callback: (progress: JobProgress) => void): void;

  /**
   * Close the transport connection.
   */
  close(): Promise<void>;
}
